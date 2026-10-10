# Plan 3: Core Simulation, Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port OpenLieroX's LX56 core simulation to Rust: its two clocks, worm movement and collision, keyboard input with OLX's key latching, digging, and the ninja rope. All seven oracle scenarios must match OLX's recorded traces bit for bit (milestone M3).

**Architecture:**
- **Crates.** Three new `#![no_std]` crates:
  - `rl-math` (layer 0): OLX's float vector, millisecond time types and math macros, exactly as OLX evaluates them;
  - `rl-terrain` (layer 1): the material grid, pixel flags, `CarveHole`, and the map hash;
  - `rl-sim` (layer 3): the world, worms, rope, physics, input, and OLX's frame and physics clocks.
- **Acceptance.** A layer-4 crate, `rl-scenario`, replays the oracle scenarios through `rl-sim` and records traces in the oracle's format. Its test is M3's acceptance test, and `rl-cli scenario` uses it for debugging divergences.

**Tech stack:** Rust 1.99, `libm` (portable `sqrtf`, `sinf`, `cosf`), plus the existing `rl-oracle`, `rl-pack` and `rl-vfs` crates. Docker is needed only in Task 1 (re-recording).

**Spec:** `docs/superpowers/specs/2026-10-10-foundation-and-local-play-design.md`. Read §5, §10 (M3) and the §17 amendments (C1–C7), which this plan implements.

## How this plan was verified

The port was written in a scratch clone and run against the oracle until every scenario matched; each mismatch was traced back to OLX's source.

**Divergences found and resolved:**
- **The FNV hash.** A mistyped FNV offset basis in the OLX hook (C7).
- **The 11 ms physics step.** OLX's `1000 / 84` integer step (C3).
- **A lossy velocity round trip.** OLX rewrites worm velocities between frames. Instrumenting OLX traced it to a Gusanos speed conversion, `GusSpeedScope`, that runs every frame (C3; `docs/porting/timing.md`).

**Then:**
- **Result.** All seven scenarios match bit for bit: `aim` 261 frames, `dig` 361, `fall` 201, `jump` 301, `rope` 401, `tap` 61, `walk` 331.
- **Replay.** The plan was replayed task by task in a fresh clone of the repository, by a script that writes exactly the code blocks below and runs exactly the commands below. Every `Expected:` line is observed output from that replay. If an output differs, stop and investigate; do not edit the expectation.

## Conventions

- **Run every command from the repository root.**
- **Steps.** A step either creates or replaces a file with the exact content shown, or runs a command.
- **Test placement.** Unit tests live in the same file as their code. The acceptance test (Task 5) and the CLI test (Task 6) come first and are watched failing.
- **`RL_CONTENT_DIR`.** Tests that need imported content read it; `cargo xtask check` sets it. Task 5 runs `cargo xtask fetch-olx-content` first.

## Global Constraints

- **Toolchain and license.** Rust 1.99, edition 2024, GPL-3.0-or-later. The crate prefix is `rl-`, and every crate has a `CLAUDE.md` and is registered in `xtask/layers.ron`.
- **`rl-math`, `rl-terrain` and `rl-sim` are `#![no_std]` + `alloc`, and build for `wasm32-unknown-unknown`.** No files, threads or clock in them.
- **Bit-exact means OLX's operation order, in `f32`:**
  - no reordering, no `mul_add`, no constant folding that changes rounding;
  - `sqrt` and trigonometry via `libm`, never `std`;
  - `TimeDiff::seconds()` is `ms as f32 * 0.001`.
- **Cite OLX for everything ported** (`file:line` at commit `d1ad953f4f20610f2648cce26c9739e7c088409d`). Keep OLX quirks and record them in `docs/porting/STATUS.md`.
- **`World::tick(&[Keys])` is the only way simulation state changes** (spec §5.1).
- **Key bits are the same everywhere:** up 1, down 2, left 4, right 8, jump 16, fire 32, rope 64, change 128 (`rl_sim::Keys`, `rl_oracle::Keys`, `OracleHooks.h`).
- **Never panic on content.** Unsupported or malformed settings are a `SettingsError`, not a panic.

## Review Focus

Conditions the spec implies that the oracle scenarios do not reach. Each is pinned by a named test in the task that owns the code.

1. **A quick tap between two physics steps.** It must still register, like OLX's key latch, and holding a key must not count as a new press. Pinned by `a_tap_is_seen_once_even_after_release` and `holding_is_down_but_once_only_after_a_press` (Task 4), and the `tap` scenario (Task 5).
2. **Settings the port does not implement.** OLX settings like wrap-around maps, or a missing or garbled value, must be a clear error rather than silently wrong physics. Pinned by `unsupported_or_broken_features_are_errors` (Task 4).
3. **Carving at the map edge.** OLX's clipping quirk must be reproduced, and nothing may index out of bounds. Pinned by `edge_clipping_keeps_olx_stamp_offset_quirk` (Task 3).
4. **Frames with no physics step**, which happen every 11th frame. The two clocks must drift exactly as in OLX. Pinned by `physics_steps_follow_the_two_clocks` (Task 5).
5. **Cloning a world**, which replays, rollback and bot matches rely on. A clone must evolve identically. Pinned by `a_clone_evolves_identically` (Task 5).

## File structure after this plan

```text
crates/rl-math/        L0 no_std  Vec2 (CVec), AbsTime/TimeDiff, sign/clamp_direct/reduce_const/reset_small
crates/rl-terrain/     L1 no_std  Terrain (flags, carve_hole, material_hash), Stamp
crates/rl-sim/         L3 no_std  settings, input (key latches), worm, rope, physics (+ get_input), world (clocks)
crates/rl-scenario/    L4         replay oracle scenarios in rl-sim; tests/oracle.rs = M3 acceptance
crates/rl-cli/                    + `scenario [name...] [--dump <frame>]`
docs/porting/timing.md            OLX's clocks, as ported
```

## After this plan

**Plan 4** covers weapons (M4): OLX's random numbers (the `RandomNumbers[]` table and glibc `rand()`), weapon selection and firing, projectiles and their 27 actions, explosions, beams, damage and bonuses. It needs new oracle scenarios that fire every weapon of both mods, and an OLX hook to set a worm's weapons.


---

### Task 1: Correct the oracle's material hash and re-record the traces

The OLX hook computed the material hash with a mistyped FNV-1a offset basis (spec §17, C7). This task fixes the constant in the patch and re-records all seven traces; only their `mh` values change. The Rust terrain hash (Task 3) uses the real FNV-1a.

**Files:**

- Modify: `tools/olx-oracle/oracle-hooks.patch` (one constant)
- Modify: `fixtures/oracle/*.jsonl.gz` (re-recorded)

**Interfaces:**

- Produces: traces whose `mh` is the standard FNV-1a 64 (offset basis `0xcbf29ce484222325`, prime `0x100000001b3`) over the material bytes, row by row.

- [ ] **Step 1: Create `tools/olx-oracle/oracle-hooks.patch`**

```text
diff --git a/include/CGameScript.h b/include/CGameScript.h
index bd9eaa4..fdbf335 100644
--- a/include/CGameScript.h
+++ b/include/CGameScript.h
@@ -178,6 +178,12 @@ public:
 	bool	getNeedCollisionInfo()		{ return needCollisionInfo; }
 	
 	size_t	getProjectileCount() const	{ return projectiles.size(); }
+	// Load-order index of a projectile type, or -1 (OracleHooks).
+	int		projectileIndex(const proj_t* p) const {
+		for(Projectiles::const_iterator it = projectiles.begin(); it != projectiles.end(); ++it)
+			if(it->second == p) return it->first;
+		return -1;
+	}
 	
 	bool	Compile(const std::string& dir);
 
diff --git a/include/CInput.h b/include/CInput.h
index 7ca1b1f..076c115 100644
--- a/include/CInput.h
+++ b/include/CInput.h
@@ -48,6 +48,11 @@
 
 struct KeyboardEvent;
 
+// Scripted key state for the rust-liero oracle test hooks (OracleHooks.h).
+// Like a keyboard binding: `pressed` mirrors nDown (a press since the last reset()
+// counts as held, CInput::Binding::isDown), `once` mirrors nDownOnce.
+struct VirtualKey { bool down; bool pressed; bool once; VirtualKey() : down(false), pressed(false), once(false) {} };
+
 class CInput {
 public:
 	CInput();
@@ -128,6 +133,9 @@ public:
 	int		wasUp(); // checks if there was an keyup-event; returns the count of up-events
 
 	std::string getEventName() { return m_EventName; }
+
+	// Test hook: when set, this input reads the scripted key instead of devices.
+	VirtualKey* virt = NULL;
 	// onlyAvailable drops bindings that can't currently fire (e.g. an absent gamepad)
 	std::string getEventName(bool onlyAvailable) const;
 
diff --git a/include/OracleHooks.h b/include/OracleHooks.h
new file mode 100644
index 0000000..906b893
--- /dev/null
+++ b/include/OracleHooks.h
@@ -0,0 +1,35 @@
+/*
+	OpenLieroX
+
+	Test hooks for the rust-liero fidelity oracle.
+	Scripted keyboard input for worms and per-frame state recording,
+	used with simSetSeed/simStep to record reference traces.
+
+	code under LGPL
+*/
+
+#ifndef __ORACLEHOOKS_H__
+#define __ORACLEHOOKS_H__
+
+#include <string>
+
+namespace OracleHooks {
+	// Key bits used by the input schedule (simLoadInputs).
+	enum {
+		KEY_UP = 1, KEY_DOWN = 2, KEY_LEFT = 4, KEY_RIGHT = 8,
+		KEY_JUMP = 16, KEY_FIRE = 32, KEY_ROPE = 64, KEY_CHANGE = 128,
+	};
+
+	// Loads "frame worm keys" lines. Frame 0 is the first frame simulated after
+	// this call. The listed worms switch to a human input handler driven by the
+	// schedule. Also aligns the LX56 physics clock with the frame clock; spawn
+	// worms after this call. Returns false and sets err on failure.
+	bool loadInputs(const std::string& file, std::string& err);
+	// Starts writing one JSON line per simulated frame to file ("off" stops).
+	bool record(const std::string& file, std::string& err);
+	// Called by the game loop around each simulated frame.
+	void beforeFrame();
+	void afterFrame();
+}
+
+#endif
diff --git a/src/client/CInput.cpp b/src/client/CInput.cpp
index 5ae81c1..fc8507a 100644
--- a/src/client/CInput.cpp
+++ b/src/client/CInput.cpp
@@ -879,6 +879,7 @@ bool CInput::Binding::isDown() const
 
 bool CInput::isDown() const
 {
+	if(virt) return virt->down || virt->pressed;
 	for(std::vector<Binding>::const_iterator it = m_bindings.begin(); it != m_bindings.end(); ++it)
 		if(it->isDown()) return true;
 	return false;
@@ -889,6 +890,7 @@ bool CInput::isDown() const
 // Returns if the input was pushed down once
 bool CInput::isDownOnce()
 {
+	if(virt) return virt->once;
 	for(std::vector<Binding>::iterator it = m_bindings.begin(); it != m_bindings.end(); ++it)
 		if(it->nDownOnce != 0) return true;
 	return false;
@@ -967,12 +969,14 @@ bool CInput::isJoystick() const {
 }
 
 bool CInput::isJoystickDown() const {
+	if(virt) return false;
 	for(std::vector<Binding>::const_iterator it = m_bindings.begin(); it != m_bindings.end(); ++it)
 		if(it->isJoystick() && it->isDown()) return true;
 	return false;
 }
 
 bool CInput::isJoystickDownOnce() const {
+	if(virt) return false;
 	for(std::vector<Binding>::const_iterator it = m_bindings.begin(); it != m_bindings.end(); ++it)
 		if(it->isJoystick() && it->nDownOnce != 0) return true;
 	return false;
@@ -1007,6 +1011,7 @@ bool CInput::usesKeyboardKey(int sym) const {
 }
 
 void CInput::reset() {
+	if(virt) virt->once = virt->pressed = false;
 	for(std::vector<Binding>::iterator it = m_bindings.begin(); it != m_bindings.end(); ++it)
 		it->reset();
 }
diff --git a/src/common/Command.cpp b/src/common/Command.cpp
index feb1223..c8aa587 100644
--- a/src/common/Command.cpp
+++ b/src/common/Command.cpp
@@ -19,6 +19,7 @@
 
 #include <limits.h>
 #include "LieroX.h"
+#include "OracleHooks.h"
 #include "Debug.h"
 #include "CServer.h"
 #include "CClient.h"
@@ -2308,6 +2309,18 @@ void Cmd_setWormInput::exec(CmdLineIntf* caller, const std::vector<std::string>&
 }
 
 
+COMMAND(simLoadInputs, "load a scripted key schedule: lines 'frame worm keys' (test hook)", "file", 1, 1);
+void Cmd_simLoadInputs::exec(CmdLineIntf* caller, const std::vector<std::string>& params) {
+	std::string err;
+	if(!OracleHooks::loadInputs(params[0], err)) caller->writeMsg(err, CNC_ERROR);
+}
+
+COMMAND(simRecord, "record one JSON line per simulated frame, or 'off' (test hook)", "file|off", 1, 1);
+void Cmd_simRecord::exec(CmdLineIntf* caller, const std::vector<std::string>& params) {
+	std::string err;
+	if(!OracleHooks::record(params[0], err)) caller->writeMsg(err, CNC_ERROR);
+}
+
 COMMAND(dumpGameState, "dump game state", "", 0, 0);
 void Cmd_dumpGameState::exec(CmdLineIntf* caller, const std::vector<std::string>& params) {
 	caller->writeMsg("GameState: " + Game::StateAsStr(game.state), CNC_DEV);
diff --git a/src/common/OracleHooks.cpp b/src/common/OracleHooks.cpp
new file mode 100644
index 0000000..4a1cb0c
--- /dev/null
+++ b/src/common/OracleHooks.cpp
@@ -0,0 +1,248 @@
+/*
+	OpenLieroX
+
+	Test hooks for the rust-liero fidelity oracle (see OracleHooks.h).
+
+	code under LGPL
+*/
+
+#include <cmath>
+#include <cstdio>
+#include <fstream>
+#include <map>
+#include <sstream>
+#include <vector>
+
+#include "LieroX.h"
+#include "OracleHooks.h"
+#include "CClient.h"
+#include "CBonus.h"
+#include "CGameScript.h"
+#include "CInput.h"
+#include "CProjectile.h"
+#include "CWormHuman.h"
+#include "FeatureList.h"
+#include "Version.h"
+#include "game/CMap.h"
+#include "game/CWorm.h"
+#include "game/Game.h"
+#include "gusanos/allegro.h"
+
+namespace OracleHooks {
+
+struct Keys {
+	VirtualKey up, down, left, right, jump, fire, rope, change, strafe, prev, next;
+};
+
+struct Event { int worm; unsigned mask; };
+
+static std::map<int, Keys> g_keys;                  // worm id -> scripted keys
+static std::map<long, std::vector<Event> > g_plan;  // frame -> key changes
+static long g_base = -1;                            // serverFrame before frame 0
+static FILE* g_rec = NULL;
+
+// Reads CWorm state that has no public getter (CWorm declares this a friend).
+struct Access {
+	static float angleSpeed(const CWorm* w) { return w->fAngleSpeed; }
+};
+
+static void writeHeader(FILE* o);
+static void writeState(long frame);
+
+static void setKey(VirtualKey& k, bool down) {
+	// A press is latched until the next getInput() resets it, like CInput's
+	// nDown (counts as held) and nDownOnce (counts as pressed once).
+	if(down && !k.down) k.once = k.pressed = true;
+	k.down = down;
+}
+
+static void apply(Keys& k, unsigned m) {
+	setKey(k.up, m & KEY_UP);
+	setKey(k.down, m & KEY_DOWN);
+	setKey(k.left, m & KEY_LEFT);
+	setKey(k.right, m & KEY_RIGHT);
+	setKey(k.jump, m & KEY_JUMP);
+	setKey(k.fire, m & KEY_FIRE);
+	setKey(k.rope, m & KEY_ROPE);
+	setKey(k.change, m & KEY_CHANGE);
+}
+
+static bool installHumanHandler(int wormId, Keys& keys, std::string& err) {
+	CWorm* w = game.wormById(wormId, false);
+	if(!w) { err = "no worm " + itoa(wormId); return false; }
+	if(!w->getLocal()) { err = "worm " + itoa(wormId) + " is not local"; return false; }
+	CWormHumanInputHandler* h = dynamic_cast<CWormHumanInputHandler*>(w->inputHandler());
+	if(!h) {
+		h = new CWormHumanInputHandler(w);
+		w->replaceInputHandlerForTest(h);
+	}
+	h->getInputUp().virt = &keys.up;
+	h->getInputDown().virt = &keys.down;
+	h->getInputLeft().virt = &keys.left;
+	h->getInputRight().virt = &keys.right;
+	h->getInputJump().virt = &keys.jump;
+	h->getInputShoot().virt = &keys.fire;
+	h->getInputRope().virt = &keys.rope;
+	h->getInputWeapon().virt = &keys.change;
+	h->getInputStrafe().virt = &keys.strafe;
+	h->getInputPrevWeapon().virt = &keys.prev;
+	h->getInputNextWeapon().virt = &keys.next;
+	return true;
+}
+
+bool loadInputs(const std::string& file, std::string& err) {
+	std::ifstream in(file.c_str());
+	if(!in) { err = "cannot open " + file; return false; }
+	g_plan.clear();
+	std::string line;
+	while(std::getline(in, line)) {
+		if(line.empty() || line[0] == '#') continue;
+		std::istringstream ls(line);
+		long frame; int worm; unsigned mask;
+		if(!(ls >> frame >> worm >> mask)) { err = "bad line: " + line; return false; }
+		g_plan[frame].push_back(Event{worm, mask});
+		if(!g_keys.count(worm) && !installHumanHandler(worm, g_keys[worm], err)) return false;
+	}
+	g_base = (long)game.serverFrame;
+	// Align the LX56 physics clock (84 Hz) with the 100 Hz frame clock, so frame 0
+	// does not catch up on however much time the setup took (as simStep does once).
+	if(cClient) cClient->fLastSimulationTime = game.simulationAbsTime();
+	return true;
+}
+
+bool record(const std::string& file, std::string& err) {
+	if(g_rec) { fclose(g_rec); g_rec = NULL; }
+	if(file == "off") return true;
+	g_rec = fopen(file.c_str(), "w");
+	if(!g_rec) { err = "cannot write " + file; return false; }
+	if(g_base < 0) g_base = (long)game.serverFrame;
+	writeHeader(g_rec);
+	writeState(-1); // the state frame 0 starts from
+	return true;
+}
+
+static long frameIndex() { return (long)game.serverFrame - g_base - 1; }
+
+void beforeFrame() {
+	if(g_base < 0) return;
+	std::map<long, std::vector<Event> >::iterator it = g_plan.find(frameIndex());
+	if(it == g_plan.end()) return;
+	for(size_t i = 0; i < it->second.size(); i++)
+		apply(g_keys[it->second[i].worm], it->second[i].mask);
+}
+
+// Floats as JSON numbers with 9 significant digits (round-trips f32), or a string if not finite.
+static void f(FILE* o, float v) {
+	if(std::isnan(v)) fputs("\"NaN\"", o);
+	else if(std::isinf(v)) fputs(v > 0 ? "\"inf\"" : "\"-inf\"", o);
+	else fprintf(o, "%.9g", v);
+}
+
+static unsigned long long materialHash() {
+	CMap* m = game.gameMap();
+	unsigned long long h = 14695981039346656037ULL; // FNV-1a 64 offset basis
+	if(!m || !m->material) return 0;
+	for(uint y = 0; y < m->GetHeight(); y++)
+		for(uint x = 0; x < m->GetWidth(); x++) {
+			h ^= m->material->line[y][x];
+			h *= 1099511628211ULL;
+		}
+	return h;
+}
+
+// JSON string with quotes and backslashes escaped.
+static void str(FILE* o, const std::string& s) {
+	fputc('"', o);
+	for(size_t i = 0; i < s.size(); i++) {
+		if(s[i] == '"' || s[i] == '\\') fputc('\\', o);
+		if((unsigned char)s[i] >= 0x20) fputc(s[i], o);
+	}
+	fputc('"', o);
+}
+
+// First line of a recording: the OLX version and every gameplay setting in effect.
+static void writeHeader(FILE* o) {
+	fputs("{\"olx\":", o);
+	str(o, GetFullGameName());
+	fputs(",\"features\":{", o);
+	for(size_t i = 0; i < featureArrayLen(); i++) {
+		if(i) fputc(',', o);
+		str(o, featureArray[i].name);
+		fputc(':', o);
+		str(o, cClient->getGameLobby()[(FeatureIndex)i].toString());
+	}
+	fputs("}}\n", o);
+}
+
+static void writeState(long frame) {
+	FILE* o = g_rec;
+	fprintf(o, "{\"f\":%ld,\"w\":[", frame);
+	bool first = true;
+	for_each_iterator(CWorm*, it, game.worms()) {
+		CWorm* w = it->get();
+		if(!first) fputc(',', o);
+		first = false;
+		const CNinjaRope& r = w->cNinjaRope.get();
+		fprintf(o, "{\"id\":%d,\"alive\":%s,\"x\":", w->getID(), w->getAlive() ? "true" : "false");
+		f(o, w->getPos().x); fputs(",\"y\":", o); f(o, w->getPos().y);
+		fputs(",\"vx\":", o); f(o, w->getVelocity().x); fputs(",\"vy\":", o); f(o, w->getVelocity().y);
+		fputs(",\"hp\":", o); f(o, w->getHealth());
+		fputs(",\"ang\":", o); f(o, w->getAngle()); fputs(",\"angv\":", o); f(o, Access::angleSpeed(w));
+		fprintf(o, ",\"face\":%d,\"move\":%d,\"ground\":%s,\"keys\":%u,\"wpn\":%d,\"slots\":[",
+			(int)w->getFaceDirectionSide(), (int)w->getMoveDirectionSide(), w->isOnGround() ? "true" : "false",
+			(unsigned)w->tState.get().asInt(), (int)w->iCurrentWeapon);
+		for(size_t i = 0; i < w->weaponSlots.get().size(); i++) {
+			const wpnslot_t& s = w->weaponSlots.get()[i];
+			if(i) fputc(',', o);
+			fprintf(o, "{\"id\":%d,\"ch\":", (int)s.WeaponId); f(o, s.Charge);
+			fprintf(o, ",\"rl\":%s,\"lf\":", s.Reloading ? "true" : "false"); f(o, s.LastFire);
+			fputc('}', o);
+		}
+		fprintf(o, "],\"rope\":{\"rel\":%s,\"sh\":%s,\"at\":%s,\"pw\":%d,\"x\":",
+			r.isReleased() ? "true" : "false", r.isShooting() ? "true" : "false",
+			r.isAttached() ? "true" : "false", r.getAttachedPlayer() ? r.getAttachedPlayer()->getID() : -1);
+		f(o, r.getPos().x); fputs(",\"y\":", o); f(o, r.getPos().y);
+		fputs(",\"vx\":", o); f(o, r.getVelocity().x); fputs(",\"vy\":", o); f(o, r.getVelocity().y);
+		fprintf(o, "},\"lives\":%d,\"kills\":%d,\"deaths\":%d,\"dirt\":%d}",
+			(int)w->getLives(), w->getKills(), w->getDeaths(), w->getDirtCount());
+	}
+	fputs("],\"p\":[", o);
+	first = true;
+	auto& ps = cClient->getProjectiles();
+	for(int i = 0; i <= ps.lastUsed(); i++) {
+		if(!ps.isUsed(i)) continue;
+		CProjectile& p = ps[i];
+		if(!first) fputc(',', o);
+		first = false;
+		fprintf(o, "{\"i\":%d,\"t\":%d,\"x\":", i, game.gameScript()->projectileIndex(p.getProjInfo()));
+		f(o, p.getPos().x); fputs(",\"y\":", o); f(o, p.getPos().y);
+		fputs(",\"vx\":", o); f(o, p.getVelocity().x); fputs(",\"vy\":", o); f(o, p.getVelocity().y);
+		fputs(",\"life\":", o); f(o, p.getLife());
+		fputc('}', o);
+	}
+	fputs("],\"b\":[", o);
+	first = true;
+	CBonus* bs = cClient->getBonusList();
+	for(int i = 0; i < MAX_BONUSES; i++) {
+		if(!bs[i].getUsed()) continue;
+		if(!first) fputc(',', o);
+		first = false;
+		fprintf(o, "{\"i\":%d,\"t\":%d,\"x\":", i, bs[i].getType());
+		f(o, bs[i].getPosition().x); fputs(",\"y\":", o); f(o, bs[i].getPosition().y);
+		fputc('}', o);
+	}
+	fprintf(o, "],\"mh\":\"%016llx\"}\n", materialHash());
+	fflush(o);
+}
+
+void afterFrame() {
+	if(g_rec) writeState(frameIndex());
+}
+
+}
+
+void CWorm::replaceInputHandlerForTest(CWormInputHandler* h) {
+	if(m_inputHandler) m_inputHandler->deleteThis();
+	m_inputHandler = h;
+	m_inputHandler->assignNetworkRole();
+}
diff --git a/src/game/CWorm.h b/src/game/CWorm.h
index ede70ca..43dafff 100644
--- a/src/game/CWorm.h
+++ b/src/game/CWorm.h
@@ -47,6 +47,7 @@
 #include <vector>
 
 
+namespace OracleHooks { struct Access; }
 class CWormInputHandler;
 class Weapon;
 class WeaponType;
@@ -166,6 +167,7 @@ class CWorm: public CGameObject {
 	friend class Client;
 	friend class Game;
 	friend class PhysicsEngine;
+friend struct OracleHooks::Access; // test hook reads fAngleSpeed
 public:
 	CWorm();
 	~CWorm();
@@ -621,6 +623,8 @@ public:
 	float&		frame()						{ return fFrame; }
 	
 	CWormInputHandler* inputHandler() { return m_inputHandler; }
+	// Test hook (OracleHooks): drive this worm with another input handler.
+	void replaceInputHandlerForTest(CWormInputHandler* h);
 	void reinitInputHandler();
 	
 	std::map< int, DamageReport> & getDamageReport() { return cDamageReport; }
diff --git a/src/game/Game.cpp b/src/game/Game.cpp
index 8603bbd..82a969c 100644
--- a/src/game/Game.cpp
+++ b/src/game/Game.cpp
@@ -8,6 +8,7 @@
  */
 
 #include "Game.h"
+#include "OracleHooks.h"
 #include "AuxLib.h"
 #include "Debug.h"
 #include "LieroX.h"
@@ -948,9 +949,11 @@ void Game::frameInner()
 				gusLogicFrame();
 			}
 
+			OracleHooks::beforeFrame();
 			cClient->Frame();
 			if(isServer())
 				cServer->Frame();
+			OracleHooks::afterFrame();
 
 			if(manualStep) manualStepBudget--;
 			simulationTime += frameDt;
```

- [ ] **Step 2: Run**

```bash
grep -c "14695981039346656037ULL" tools/olx-oracle/oracle-hooks.patch
```

Expected: output contains `1`. The corrected offset basis is in the patch.

- [ ] **Step 3: Run**

```bash
cargo xtask oracle-record
```

Expected: output contains `ok   walk: recorded 331 frames`. The patch changed, so the image is rebuilt first (`building rl-olx-oracle:...`, a few minutes). Then all seven scenarios record, each twice and identically.

- [ ] **Step 4: Run**

```bash
cargo nextest run -p rl-oracle
```

Expected: output contains `28 passed`. The fixture tests still pass: only the `mh` values changed.

- [ ] **Step 5: Run**

```bash
cargo xtask check --offline
```

Expected: output contains `check passed`.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "Oracle: use the real FNV-1a offset basis for mh"
```


---

### Task 2: `rl-math`: OLX's vector, time and math helpers

The bit-exact numeric base of the simulation: `CVec`, millisecond time, and OLX's macros.

**Files:**

- Modify: `Cargo.toml` (workspace deps `rl-math`, `rl-terrain`, `rl-sim`, `rl-scenario`, `libm`)
- Create: `crates/rl-math/{Cargo.toml,CLAUDE.md}`, `crates/rl-math/src/{lib,time,vec2}.rs`
- Modify: `xtask/layers.ron` (register `rl-math`: layer 0, `no_std`, `wasm`)

**Interfaces:**

- Produces: `rl_math::Vec2 { x: f32, y: f32 }`: `new`, `ZERO`, `length2`, `length` (`libm::sqrtf`), `normalize`; `+ - neg`; `* f32`, `/ f32`; `+= -= *=`.
- Produces: `AbsTime(pub u64)` and `TimeDiff(pub u64)` (ms). `TimeDiff::seconds()` is `ms as f32 * 0.001`; `TimeDiff::scaled(f32)` truncates; `AbsTime + TimeDiff`, `AbsTime - AbsTime -> TimeDiff` (saturating).
- Produces: `sign(f32) -> i32`, `clamp_direct(&mut f32, lo, hi) -> i32`, `reduce_const(&mut f32, f32)`, `reset_small(&mut f32, f32)`, `PI_F32`.

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[workspace]
resolver = "3"
members = ["crates/*", "xtask"]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "GPL-3.0-or-later"
rust-version = "1.99"
publish = false

[workspace.dependencies]
rl-math = { path = "crates/rl-math" }
rl-data = { path = "crates/rl-data" }
rl-terrain = { path = "crates/rl-terrain" }
rl-sim = { path = "crates/rl-sim" }
rl-scenario = { path = "crates/rl-scenario" }
rl-vfs = { path = "crates/rl-vfs" }
rl-pack = { path = "crates/rl-pack" }
rl-import-lgs = { path = "crates/rl-import-lgs" }
rl-import-lxl = { path = "crates/rl-import-lxl" }
rl-import-gfx = { path = "crates/rl-import-gfx" }
rl-oracle = { path = "crates/rl-oracle" }
serde = { version = "1.0.229", default-features = false, features = ["derive", "alloc"] }
ron = "0.12.2"
libm = "0.2.16"
serde_json = "1.0.151"
png = "0.18.1"
flate2 = "1.1.10"
sha2 = "0.11.0"
hex = "0.4.3"
thiserror = "2.0.21"
anyhow = "1.0.104"
clap = { version = "4.6.7", features = ["derive"] }
cargo_metadata = "0.23.1"
syn = { version = "2.0.105", features = ["full"] }
prettyplease = "0.2.37"
quote = "1"
proptest = "1.11.0"
insta = "1.49.0"
tempfile = "3"

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }

[profile.dev]
opt-level = 1

[profile.dev.package."*"]
opt-level = 3
```

- [ ] **Step 2: Create `crates/rl-math/Cargo.toml`**

```toml
[package]
name = "rl-math"
description = "Exact ports of OpenLieroX's float vector, time types and math helpers."
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
libm.workspace = true

[lints]
workspace = true
```

- [ ] **Step 3: Create `crates/rl-math/CLAUDE.md`**

```markdown
# rl-math

Bit-exact ports of OLX math: `Vec2` (`CVec`), `AbsTime`/`TimeDiff` (whole
milliseconds, `seconds() = ms * 0.001f`), and the macros `SIGN`, `CLAMP_DIRECT`,
`REDUCE_CONST`, `RESET_SMALL`.

- **Layer 0, `#![no_std]`.** Builds for `wasm32-unknown-unknown`.
- **`f32` only, in OLX's operation order.** Never reorder an expression, fold
  constants differently, or use `mul_add`: fidelity is checked bit for bit by
  the oracle (`fixtures/oracle`).
- **`sqrt` and trigonometry use the `libm` crate**, never `std`, so every platform gets the same bits.

Test: `cargo nextest run -p rl-math`.
```

- [ ] **Step 4: Create `crates/rl-math/src/lib.rs`**

```rust
//! Bit-exact ports of the OpenLieroX math the simulation depends on.
//!
//! `#![no_std]`: everything here must give the same bits on every platform,
//! including `wasm32`. Square roots and trigonometry go through the `libm`
//! crate, never through the platform's math library.
#![no_std]

mod time;
mod vec2;

pub use time::{AbsTime, TimeDiff};
pub use vec2::Vec2;

/// OLX `SIGN` (`include/MathLib.h:60`): -1, 0 or 1.
pub fn sign(x: f32) -> i32 {
    if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }
}

/// OLX `CLAMP_DIRECT` (`include/MathLib.h:71-79`): clamps `v` in place and
/// returns -1, 1 or 0 for "clamped low", "clamped high", "unchanged".
pub fn clamp_direct(v: &mut f32, lo: f32, hi: f32) -> i32 {
    if *v < lo {
        *v = lo;
        -1
    } else if *v > hi {
        *v = hi;
        1
    } else {
        0
    }
}

/// OLX `REDUCE_CONST` (`include/MathLib.h:80-81`): moves `v` towards 0 by
/// `amount` without crossing it.
pub fn reduce_const(v: &mut f32, amount: f32) {
    if *v > 0.0 {
        *v = if 0.0 > *v - amount { 0.0 } else { *v - amount };
    } else if *v < 0.0 {
        *v = if 0.0 < *v + amount { 0.0 } else { *v + amount };
    }
}

/// OLX `RESET_SMALL` (`include/MathLib.h:82-83`): sets `v` to 0 if `|v| < limit`.
pub fn reset_small(v: &mut f32, limit: f32) {
    if (*v > 0.0 && *v < limit) || (*v < 0.0 && *v > -limit) {
        *v = 0.0;
    }
}

/// OLX's `PI` as the float `(float)PI` (`include/MathLib.h:31`).
pub const PI_F32: f32 = core::f32::consts::PI;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macros_match_olx() {
        assert_eq!((sign(-0.5), sign(0.0), sign(-0.0), sign(3.0)), (-1, 0, 0, 1));
        let mut v = 7.0;
        assert_eq!(clamp_direct(&mut v, -1.0, 5.0), 1);
        assert_eq!(v, 5.0);
        let mut v = -3.0;
        reduce_const(&mut v, 2.0);
        assert_eq!(v, -1.0);
        reduce_const(&mut v, 2.0);
        assert_eq!(v, 0.0);
        let mut v = 4.9;
        reset_small(&mut v, 5.0);
        assert_eq!(v, 0.0);
        let mut v = -5.0;
        reset_small(&mut v, 5.0);
        assert_eq!(v, -5.0);
    }

    #[test]
    fn pi_is_olx_float_pi() {
        // OLX's PI is the double literal 3.14159265358979323846, cast with (float)PI.
        assert_eq!(PI_F32, core::f64::consts::PI as f32);
    }
}
```

- [ ] **Step 5: Create `crates/rl-math/src/time.rs`**

```rust
//! OLX's `AbsTime` and `TimeDiff` (`include/olx-types.h:29-100`): whole
//! milliseconds in a `u64`, converted to seconds as `ms * 0.001f`.

use core::ops::{Add, AddAssign, Sub};

/// A point in simulation time, in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AbsTime(pub u64);

/// A duration, in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeDiff(pub u64);

impl TimeDiff {
    /// `TimeDiff::seconds()`: `timeDiff * 0.001f`.
    pub fn seconds(self) -> f32 {
        self.0 as f32 * 0.001
    }

    /// `TimeDiff * float`, truncated back to whole milliseconds like OLX's `(Uint64)(timeDiff * f)`.
    pub fn scaled(self, f: f32) -> TimeDiff {
        TimeDiff((self.0 as f32 * f) as u64)
    }
}

impl Add<TimeDiff> for AbsTime {
    type Output = AbsTime;
    fn add(self, d: TimeDiff) -> AbsTime {
        AbsTime(self.0 + d.0)
    }
}

impl AddAssign<TimeDiff> for AbsTime {
    fn add_assign(&mut self, d: TimeDiff) {
        self.0 += d.0;
    }
}

impl Sub for AbsTime {
    type Output = TimeDiff;
    /// OLX asserts `self >= other`; this saturates at 0 instead of panicking.
    fn sub(self, other: AbsTime) -> TimeDiff {
        TimeDiff(self.0.saturating_sub(other.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_use_float_multiplication() {
        assert_eq!(TimeDiff(11).seconds(), 11_f32 * 0.001);
        // 20 ms is `20 * 0.001f`, which is not the same float as `0.02f`.
        assert_eq!(TimeDiff(20).seconds(), 20_f32 * 0.001);
        assert_ne!(TimeDiff(20).seconds(), 0.02_f32);
        assert_eq!((AbsTime(30) - AbsTime(10)).0, 20);
        assert_eq!(AbsTime(5) + TimeDiff(6), AbsTime(11));
        assert_eq!(TimeDiff(11).scaled(1.0 / 0.5), TimeDiff(22));
    }
}
```

- [ ] **Step 6: Create `crates/rl-math/src/vec2.rs`**

```rust
//! OLX `CVec` = `VectorD2<float>` (`include/CVec.h`). Every operation is a
//! plain `f32` operation in the same order as the C++, so results match bit
//! for bit. Scalars are always `f32`: OLX code that scales by an `int`
//! converts it to `float` first, which is what callers do here too.

use core::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub, SubAssign};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// `GetLength2`: `x*x + y*y`.
    pub fn length2(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    /// `GetLength`: `sqrtf(x*x + y*y)`.
    pub fn length(self) -> f32 {
        libm::sqrtf(self.x * self.x + self.y * self.y)
    }

    /// `Normalize`: `*this / GetLength()` (no zero check, like OLX).
    pub fn normalize(self) -> Vec2 {
        self / self.length()
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}

impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

impl Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f32) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}

impl Div<f32> for Vec2 {
    type Output = Vec2;
    fn div(self, s: f32) -> Vec2 {
        Vec2::new(self.x / s, self.y / s)
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, o: Vec2) {
        *self = *self + o;
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, o: Vec2) {
        *self = *self - o;
    }
}

impl MulAssign<f32> for Vec2 {
    fn mul_assign(&mut self, s: f32) {
        *self = *self * s;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operations_are_plain_f32() {
        let v = Vec2::new(3.0, 4.0);
        assert_eq!(v.length2(), 25.0);
        assert_eq!(v.length(), 5.0);
        assert_eq!(v.normalize(), Vec2::new(0.6, 0.8));
        assert_eq!((v * 2.0 - Vec2::new(1.0, 1.0)) / 2.0, Vec2::new(2.5, 3.5));
        assert_eq!(-v, Vec2::new(-3.0, -4.0));
        let mut w = v;
        w += Vec2::new(1.0, 0.0);
        w *= 0.5;
        assert_eq!(w, Vec2::new(2.0, 2.0));
    }

    #[test]
    fn sqrt_is_correctly_rounded() {
        assert_eq!(Vec2::new(1.0, 1.0).length(), core::f32::consts::SQRT_2);
    }
}
```

- [ ] **Step 7: Create `xtask/layers.ron`**

```ron
// Crate layering (spec §4). A crate may depend on crates in lower layers, and on
// same-layer crates only when the pair is listed in `same_layer`. Every
// workspace crate must be listed here. `no_std` crates must declare
// #![no_std]; `wasm` crates must build for wasm32-unknown-unknown.
(
    layers: {
        "rl-math": 0,
        "rl-data": 1,
        "rl-vfs": 1,
        "rl-pack": 2,
        "rl-import-lgs": 2,
        "rl-import-lxl": 2,
        "rl-import-gfx": 2,
        "rl-oracle": 2,
        "rl-cli": 6,
        "xtask": 6,
    },
    same_layer: [
        ("rl-import-lgs", "rl-pack"),
        ("rl-import-lxl", "rl-pack"),
        ("rl-import-gfx", "rl-pack"),
    ],
    no_std: ["rl-math", "rl-data"],
    wasm: ["rl-math", "rl-data", "rl-vfs", "rl-pack", "rl-import-lgs", "rl-import-lxl", "rl-import-gfx"],
)
```

- [ ] **Step 8: Run**

```bash
cargo nextest run -p rl-math
```

Expected: output contains `5 passed`. Includes `seconds_use_float_multiplication`, which pins that 20 ms is `20 * 0.001f`, not `0.02f`.

- [ ] **Step 9: Run**

```bash
cargo xtask check --offline
```

Expected: output contains `check passed`. `wasm` also builds `rl-math`.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "rl-math: add OLX vector, time and math helpers"
```


---

### Task 3: `rl-terrain`: material grid, carving and map hash

The terrain the simulation collides with: OLX's pixel flags, `CarveHole` for the material grid (with its edge-clipping quirk), and the oracle's `mh` hash.

**Files:**

- Create: `crates/rl-terrain/{Cargo.toml,CLAUDE.md}`, `crates/rl-terrain/src/{lib,stamp}.rs`
- Modify: `xtask/layers.ron` (register `rl-terrain`: layer 1, `no_std`, `wasm`; allow `rl-terrain` → `rl-data`)

**Interfaces:**

- Produces: `Terrain::from_grid(&MaterialGrid)`, `width()`, `height()`, `material(x: i64, y: i64) -> Option<Material>`, `flags(x, y) -> u8` (rock outside the map), `carve_hole(&Stamp, x: i32, y: i32) -> u32`, `material_hash() -> u64`.
- Produces: `PX_EMPTY = 1`, `PX_DIRT = 2`, `PX_ROCK = 4`; `Stamp::from_image(&RgbaImage)`, `width()`, `height()`, `get(x, y) -> StampPixel { Carve | Skip | Paint(Rgba) }`.

- [ ] **Step 1: Create `crates/rl-terrain/Cargo.toml`**

```toml
[package]
name = "rl-terrain"
description = "The simulation's terrain: material grid, OLX pixel flags, hole carving, material hash."
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
rl-data.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Create `crates/rl-terrain/CLAUDE.md`**

```markdown
# rl-terrain

The simulation's terrain (spec §5.4): one `Material` byte per world pixel.

- **Layer 1, `#![no_std]` + `alloc`.**
- `flags(x, y)` is OLX's `GetPixelFlag`: rock outside the map.
- `carve_hole(stamp, x, y)` ports `CMap::CarveHole` for the material grid,
  including OLX's edge-clipping quirk: the stamp is read from its top-left
  corner even when its left or top part is clipped.
- `material_hash()` is the oracle's `mh`: FNV-1a 64 over the material bytes.
- Colour art is not here. Rendering (M5) applies the same stamps to the images.

Test: `cargo nextest run -p rl-terrain`.
```

- [ ] **Step 3: Create `crates/rl-terrain/src/lib.rs`**

```rust
//! Terrain as the simulation sees it (spec §5.4): one material byte per world
//! pixel, OLX's pixel-flag view of it, and hole carving.
//!
//! `#![no_std]` + `alloc`. Colour art is not here: carving reports what changed
//! and the renderer (M5) applies the same stamp to the images.
#![no_std]

extern crate alloc;

mod stamp;

use alloc::vec::Vec;

use rl_data::level::{Material, MaterialGrid};
pub use stamp::{Stamp, StampPixel};

/// OLX pixel flags (`src/level/LXMapFlags.h`).
pub const PX_EMPTY: u8 = 0x01;
pub const PX_DIRT: u8 = 0x02;
pub const PX_ROCK: u8 = 0x04;

/// The material grid the simulation collides with and carves into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Terrain {
    width: u32,
    height: u32,
    /// Material indices (`Material as u8`), row-major.
    cells: Vec<u8>,
}

impl Terrain {
    pub fn from_grid(grid: &MaterialGrid) -> Self {
        Self {
            width: grid.width(),
            height: grid.height(),
            cells: grid.cells().iter().map(|m| *m as u8).collect(),
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The material at `(x, y)`, or `None` outside the map.
    pub fn material(&self, x: i64, y: i64) -> Option<Material> {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return None;
        }
        Material::from_index(self.cells[y as usize * self.width as usize + x as usize])
    }

    /// OLX `CMap::GetPixelFlag` (`src/game/CMap.h:291-302`) without wrap-around:
    /// outside the map is rock; inside, `Material::toLxFlags`.
    pub fn flags(&self, x: i64, y: i64) -> u8 {
        match self.material(x, y) {
            None | Some(Material::Rock) => PX_ROCK,
            Some(Material::Dirt) => PX_DIRT,
            Some(Material::Background) => PX_EMPTY,
        }
    }

    /// OLX `CMap::CarveHole` (`src/common/CMap.cpp:1005-1105`) for the material
    /// grid: centres `stamp` on `(pos_x, pos_y)` (already truncated to ints by
    /// the caller), turns dirt under magenta stamp pixels into background, and
    /// returns how many cells it carved.
    ///
    /// Kept quirk: after clipping at the map edge, OLX still reads the stamp
    /// from its top-left corner (the stamp is not offset by the clipped amount).
    pub fn carve_hole(&mut self, stamp: &Stamp, pos_x: i32, pos_y: i32) -> u32 {
        let (w, h) = (stamp.width() as i32, stamp.height() as i32);
        let (mut x, mut y, mut cw, mut ch) = (pos_x - w / 2, pos_y - h / 2, w, h);
        if !clip(&mut x, &mut cw, self.width as i32) || !clip(&mut y, &mut ch, self.height as i32) {
            return 0;
        }
        let mut carved = 0;
        for row in 0..ch {
            for col in 0..cw {
                let i = (y + row) as usize * self.width as usize + (x + col) as usize;
                if self.cells[i] == Material::Dirt as u8
                    && stamp.get(col as u32, row as u32) == StampPixel::Carve
                {
                    self.cells[i] = Material::Background as u8;
                    carved += 1;
                }
            }
        }
        carved
    }

    /// FNV-1a 64 over the material indices, row by row: the oracle's `mh`.
    pub fn material_hash(&self) -> u64 {
        self.cells
            .iter()
            .fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3))
    }
}

/// OLX `OLXRect::clipWith` on one axis against `[0, limit)`: returns false if nothing is left.
fn clip(start: &mut i32, len: &mut i32, limit: i32) -> bool {
    let end = *start + *len;
    *start = (*start).max(0);
    let end = end.min(limit).max(*start);
    *len = end - *start;
    *len > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use rl_data::image::{Rgba, RgbaImage};

    fn grid(w: u32, h: u32, m: Material) -> Terrain {
        Terrain::from_grid(&MaterialGrid::new(w, h, m))
    }

    fn stamp(w: u32, h: u32, carve: &[(u32, u32)]) -> Stamp {
        let mut img = RgbaImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                img.set(x, y, Rgba::BLACK);
            }
        }
        for &(x, y) in carve {
            img.set(x, y, Rgba::MAGENTA);
        }
        Stamp::from_image(&img)
    }

    #[test]
    fn flags_follow_olx() {
        let mut t = grid(2, 1, Material::Dirt);
        t.cells[1] = Material::Background as u8;
        assert_eq!((t.flags(0, 0), t.flags(1, 0)), (PX_DIRT, PX_EMPTY));
        assert_eq!((t.flags(-1, 0), t.flags(2, 0), t.flags(0, 1)), (PX_ROCK, PX_ROCK, PX_ROCK));
    }

    #[test]
    fn carves_only_dirt_under_magenta() {
        let mut t = grid(5, 5, Material::Dirt);
        t.cells[12 + 1] = Material::Rock as u8; // (3,2)
        let s = stamp(3, 3, &[(0, 1), (1, 1), (2, 1), (1, 0)]);
        assert_eq!(t.carve_hole(&s, 2, 2), 3); // (3,2) is rock
        assert_eq!(t.material(1, 2), Some(Material::Background));
        assert_eq!(t.material(2, 1), Some(Material::Background));
        assert_eq!(t.material(3, 2), Some(Material::Rock));
        assert_eq!(t.material(2, 3), Some(Material::Dirt));
        assert_eq!(t.carve_hole(&s, 2, 2), 0, "nothing left to carve");
    }

    #[test]
    fn edge_clipping_keeps_olx_stamp_offset_quirk() {
        // Stamp centred at x=0: the left column is clipped away, but OLX still
        // reads the stamp from its column 0, so stamp column 0 lands on map x=0.
        let mut t = grid(3, 1, Material::Dirt);
        let s = stamp(3, 1, &[(0, 0)]);
        assert_eq!(t.carve_hole(&s, 0, 0), 1);
        assert_eq!(t.material(0, 0), Some(Material::Background));
        assert_eq!(t.material(1, 0), Some(Material::Dirt));
        assert_eq!(t.carve_hole(&s, -10, 0), 0, "fully outside");
    }

    #[test]
    fn hash_changes_with_material() {
        let mut t = grid(2, 2, Material::Dirt);
        let before = t.material_hash();
        t.cells[0] = Material::Background as u8;
        assert_ne!(before, t.material_hash());
        assert_eq!(
            grid(1, 1, Material::Rock).material_hash(),
            (0xcbf2_9ce4_8422_2325_u64).wrapping_mul(0x100_0000_01b3)
        );
        let _ = vec![0u8; 0];
    }
}
```

- [ ] **Step 4: Create `crates/rl-terrain/src/stamp.rs`**

```rust
//! Carving stamps: a theme's hole images classified the way OLX reads them.

use alloc::vec::Vec;

use rl_data::image::{Rgba, RgbaImage};

/// What a stamp pixel does when carving (`CMap::CarveHole`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StampPixel {
    /// Magenta: turn dirt into background.
    Carve,
    /// Black: leave the map alone.
    Skip,
    /// Any other colour: dirt stays, but the renderer paints this colour (the dirt edge).
    Paint(Rgba),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stamp {
    width: u32,
    height: u32,
    pixels: Vec<StampPixel>,
}

impl Stamp {
    pub fn from_image(img: &RgbaImage) -> Self {
        let mut pixels = Vec::with_capacity(img.width() as usize * img.height() as usize);
        for y in 0..img.height() {
            for x in 0..img.width() {
                let c = img.get(x, y);
                pixels.push(if c.same_rgb(Rgba::MAGENTA) {
                    StampPixel::Carve
                } else if c.same_rgb(Rgba::BLACK) {
                    StampPixel::Skip
                } else {
                    StampPixel::Paint(c)
                });
            }
        }
        Self { width: img.width(), height: img.height(), pixels }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Panics if out of bounds.
    pub fn get(&self, x: u32, y: u32) -> StampPixel {
        self.pixels[(y * self.width + x) as usize]
    }
}
```

- [ ] **Step 5: Replace `xtask/layers.ron`**

```ron
// Crate layering (spec §4). A crate may depend on crates in lower layers, and on
// same-layer crates only when the pair is listed in `same_layer`. Every
// workspace crate must be listed here. `no_std` crates must declare
// #![no_std]; `wasm` crates must build for wasm32-unknown-unknown.
(
    layers: {
        "rl-math": 0,
        "rl-data": 1,
        "rl-terrain": 1,
        "rl-vfs": 1,
        "rl-pack": 2,
        "rl-import-lgs": 2,
        "rl-import-lxl": 2,
        "rl-import-gfx": 2,
        "rl-oracle": 2,
        "rl-cli": 6,
        "xtask": 6,
    },
    same_layer: [
        ("rl-terrain", "rl-data"),
        ("rl-import-lgs", "rl-pack"),
        ("rl-import-lxl", "rl-pack"),
        ("rl-import-gfx", "rl-pack"),
    ],
    no_std: ["rl-math", "rl-data", "rl-terrain"],
    wasm: ["rl-math", "rl-data", "rl-terrain", "rl-vfs", "rl-pack", "rl-import-lgs", "rl-import-lxl", "rl-import-gfx"],
)
```

- [ ] **Step 6: Run**

```bash
cargo nextest run -p rl-terrain
```

Expected: output contains `4 passed`. Includes `edge_clipping_keeps_olx_stamp_offset_quirk`.

- [ ] **Step 7: Run**

```bash
cargo xtask check --offline
```

Expected: output contains `check passed`.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "rl-terrain: add material grid, carving and map hash"
```


---

### Task 4: `rl-sim`: settings and key latches

The simulation crate starts with its inputs: typed settings read from OLX's feature values, and keys latched the way OLX's `CInput` latches them between physics steps.

**Files:**

- Create: `crates/rl-sim/{Cargo.toml,CLAUDE.md}`, `crates/rl-sim/src/{lib,settings,input}.rs`
- Modify: `xtask/layers.ron` (register `rl-sim`: layer 3, `no_std`, `wasm`)

**Interfaces:**

- Produces: `SimSettings` (all fields public), `SimSettings::lx56()`, and `SimSettings::from_olx_features(&BTreeMap<String,String>) -> Result<SimSettings, SettingsError>`; `SettingsError(pub String)`.
- Produces: `Keys(pub u8)`, with constants `UP 1, DOWN 2, LEFT 4, RIGHT 8, JUMP 16, FIRE 32, ROPE 64, CHANGE 128` (the same bits as `rl_oracle::Keys`).
- Produces: `KeyLatch` (`set(down)`, `is_down()`, `is_once()`, `reset()`); `Latches { up, down, left, right, jump, fire, rope, change }` (all `KeyLatch`) with `set(Keys)` and `reset()`.

- [ ] **Step 1: Create `crates/rl-sim/Cargo.toml`**

```toml
[package]
name = "rl-sim"
description = "The deterministic game simulation: LX56 worm physics, ninja rope, input, frame and physics clocks."
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
rl-math.workspace = true
rl-data.workspace = true
rl-terrain.workspace = true
libm.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Create `crates/rl-sim/CLAUDE.md`**

```markdown
# rl-sim

The deterministic simulation (spec §5), a port of OLX's LX56 physics at
commit `d1ad953f`. The oracle traces in `fixtures/oracle` are its acceptance
tests, and they must match bit for bit (`tests/oracle.rs`).

- **Layer 3, `#![no_std]` + `alloc`.** `World::tick(&[Keys])` is the only way state changes.
- **Two clocks.** `tick` is one 100 Hz frame. Inside it, LX56 physics runs in
  `1000 / LX56PhysicsFPS` ms sub-steps (11 ms by default) while the physics
  clock is behind the frame clock, so some frames run no sub-step.
- **Input** (`input.rs`): keys are latched between reads exactly like OLX's
  `CInput`. See `docs/porting/STATUS.md`.
- **Port, don't improve.** Keep OLX's operation order and quirks; cite the
  OLX source for each function. Any change must keep `tests/oracle.rs` green.
- **Not here yet (M4):** weapons, projectiles, bonuses, damage, `rand()`.

Test: `cargo nextest run -p rl-sim`. The oracle test needs imported content;
`cargo xtask check` sets `RL_CONTENT_DIR`.
```

- [ ] **Step 3: Create `crates/rl-sim/src/lib.rs`**

```rust
//! The deterministic simulation (spec §5), ported from OpenLieroX's LX56
//! physics at commit `d1ad953f` and verified against the oracle traces in
//! `fixtures/oracle` (bit-exact).
//!
//! `#![no_std]` + `alloc`: no files, threads or clock. [`World::tick`] is the
//! only way state changes.
//!
//! Scope (M3): worm movement and collision, keyboard input, digging, the ninja
//! rope, and OLX's two clocks. Weapons, projectiles, bonuses and damage come in M4.
#![no_std]

extern crate alloc;

mod input;
mod settings;

pub use input::{KeyLatch, Keys, Latches};
pub use settings::{SettingsError, SimSettings};
```

- [ ] **Step 4: Create `crates/rl-sim/src/settings.rs`**

```rust
//! The gameplay settings the simulation reads (OLX `FeatureList`).

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;

/// Typed gameplay settings. [`SimSettings::lx56`] is OLX's "LieroX 0.56"
/// preset with the Classic mod, which is what the oracle traces use.
#[derive(Clone, Debug, PartialEq)]
pub struct SimSettings {
    /// `LX56PhysicsFPS`: the physics step is `1000 / fps` whole milliseconds.
    pub lx56_physics_fps: i32,
    pub game_speed: f32,
    pub game_speed_only_for_projs: bool,
    pub worm_ground_speed: f32,
    pub worm_air_speed: f32,
    pub worm_max_ground_move_speed: f32,
    pub worm_max_air_move_speed: f32,
    pub worm_jump_force: f32,
    pub worm_gravity: f32,
    pub worm_air_friction: f32,
    pub worm_ground_friction: f32,
    pub worm_ground_stop_speed: f32,
    pub worm_speed_factor: f32,
    pub relative_air_jump: bool,
    pub relative_air_jump_delay: f32,
    pub jump_to_aim_dir: bool,
    pub full_aim_angle: bool,
    pub rope_max_length: i32,
    pub rope_rest_length: i32,
    pub rope_strength: f32,
    pub rope_speed: f32,
    pub rope_gravity: f32,
    pub rope_falling_gravity: f32,
    pub rope_add_parent_speed: bool,
    pub rope_can_attach_worm: bool,
    pub ninjarope_precision: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsError(pub String);

impl SimSettings {
    /// OLX's "LieroX 0.56" values with the Classic mod (as in the oracle trace headers).
    pub fn lx56() -> Self {
        Self {
            lx56_physics_fps: 84,
            game_speed: 1.0,
            game_speed_only_for_projs: false,
            worm_ground_speed: 8.0,
            worm_air_speed: 4.0,
            worm_max_ground_move_speed: 30.0,
            worm_max_air_move_speed: 30.0,
            worm_jump_force: -65.0,
            worm_gravity: 100.0,
            worm_air_friction: 0.0,
            worm_ground_friction: 0.1,
            worm_ground_stop_speed: 5.0,
            worm_speed_factor: 1.0,
            relative_air_jump: false,
            relative_air_jump_delay: 0.7,
            jump_to_aim_dir: false,
            full_aim_angle: false,
            rope_max_length: 300,
            rope_rest_length: 27,
            rope_strength: 4.5,
            rope_speed: 250.0,
            rope_gravity: 100.0,
            rope_falling_gravity: 150.0,
            rope_add_parent_speed: false,
            rope_can_attach_worm: true,
            ninjarope_precision: 1.0,
        }
    }

    /// Reads the settings from OLX feature values (a trace header's `features`).
    /// Fails if a needed feature is missing or malformed, or if OLX had a mode
    /// on that this simulation does not implement (wrap-around maps, Gusanos
    /// worm physics, worm friction, non-LX56 aim).
    pub fn from_olx_features(f: &BTreeMap<String, String>) -> Result<Self, SettingsError> {
        let get = |k: &str| f.get(k).ok_or_else(|| SettingsError(format!("feature {k} missing")));
        let num = |k: &str| -> Result<f32, SettingsError> {
            get(k)?.parse::<f32>().map_err(|_| SettingsError(format!("feature {k} is not a number")))
        };
        let int = |k: &str| -> Result<i32, SettingsError> {
            get(k)?.parse::<i32>().map_err(|_| SettingsError(format!("feature {k} is not an integer")))
        };
        let flag = |k: &str| -> Result<bool, SettingsError> {
            match get(k)?.as_str() {
                "true" => Ok(true),
                "false" => Ok(false),
                _ => Err(SettingsError(format!("feature {k} is not a boolean"))),
            }
        };
        for (k, want) in [("InfiniteMap", false), ("GusanosWormPhysics", false), ("ForceLX56Aim", true)] {
            if flag(k)? != want {
                return Err(SettingsError(format!("feature {k}={} is not supported", !want)));
            }
        }
        if num("WormFriction")? != 0.0 {
            return Err(SettingsError("feature WormFriction > 0 is not supported".into()));
        }
        Ok(Self {
            lx56_physics_fps: int("LX56PhysicsFPS")?,
            game_speed: num("GameSpeed")?,
            game_speed_only_for_projs: flag("GameSpeedOnlyForProjs")?,
            worm_ground_speed: num("WormGroundSpeed")?,
            worm_air_speed: num("WormAirSpeed")?,
            worm_max_ground_move_speed: num("WormMaxGroundMoveSpeed")?,
            worm_max_air_move_speed: num("WormMaxAirMoveSpeed")?,
            worm_jump_force: num("WormJumpForce")?,
            worm_gravity: num("WormGravity")?,
            worm_air_friction: num("WormAirFriction")?,
            worm_ground_friction: num("WormGroundFriction")?,
            worm_ground_stop_speed: num("WormGroundStopSpeed")?,
            worm_speed_factor: num("WormSpeedFactor")?,
            relative_air_jump: flag("RelativeAirJump")?,
            relative_air_jump_delay: num("RelativeAirJumpDelay")?,
            jump_to_aim_dir: flag("JumpToAimDir")?,
            full_aim_angle: flag("FullAimAngle")?,
            rope_max_length: int("RopeMaxLength")?,
            rope_rest_length: int("RopeRestLength")?,
            rope_strength: num("RopeStrength")?,
            rope_speed: num("RopeSpeed")?,
            rope_gravity: num("RopeGravity")?,
            rope_falling_gravity: num("RopeFallingGravity")?,
            rope_add_parent_speed: flag("RopeAddParentSpeed")?,
            rope_can_attach_worm: flag("RopeCanAttachWorm")?,
            ninjarope_precision: num("NinjaropePrecision")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    /// The relevant features of the oracle trace headers (`fixtures/oracle/*.jsonl.gz`).
    fn olx_header() -> BTreeMap<String, String> {
        [
            ("InfiniteMap", "false"),
            ("GusanosWormPhysics", "false"),
            ("ForceLX56Aim", "true"),
            ("WormFriction", "0"),
            ("LX56PhysicsFPS", "84"),
            ("GameSpeed", "1"),
            ("GameSpeedOnlyForProjs", "false"),
            ("WormGroundSpeed", "8"),
            ("WormAirSpeed", "4"),
            ("WormMaxGroundMoveSpeed", "30"),
            ("WormMaxAirMoveSpeed", "30"),
            ("WormJumpForce", "-65"),
            ("WormGravity", "100"),
            ("WormAirFriction", "0"),
            ("WormGroundFriction", "0.1"),
            ("WormGroundStopSpeed", "5"),
            ("WormSpeedFactor", "1"),
            ("RelativeAirJump", "false"),
            ("RelativeAirJumpDelay", "0.7"),
            ("JumpToAimDir", "false"),
            ("FullAimAngle", "false"),
            ("RopeMaxLength", "300"),
            ("RopeRestLength", "27"),
            ("RopeStrength", "4.5"),
            ("RopeSpeed", "250"),
            ("RopeGravity", "100"),
            ("RopeFallingGravity", "150"),
            ("RopeAddParentSpeed", "false"),
            ("RopeCanAttachWorm", "true"),
            ("NinjaropePrecision", "1"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn olx_header_gives_the_lx56_defaults() {
        assert_eq!(SimSettings::from_olx_features(&olx_header()), Ok(SimSettings::lx56()));
    }

    #[test]
    fn unsupported_or_broken_features_are_errors() {
        let mut f = olx_header();
        f.insert("InfiniteMap".into(), "true".into());
        assert!(SimSettings::from_olx_features(&f).unwrap_err().0.contains("InfiniteMap"));
        let mut f = olx_header();
        f.remove("RopeSpeed");
        assert_eq!(SimSettings::from_olx_features(&f).unwrap_err().0, "feature RopeSpeed missing");
        let mut f = olx_header();
        f.insert("WormGravity".into(), "heavy".into());
        assert!(SimSettings::from_olx_features(&f).unwrap_err().0.contains("not a number"));
    }
}
```

- [ ] **Step 5: Create `crates/rl-sim/src/input.rs`**

```rust
//! Keyboard input with OLX's latching (`src/client/CInput.cpp`).
//!
//! OLX reads keys once per physics sub-step (`getInput`), not once per frame,
//! and keeps two latches per key until that read ends with `CInput::reset()`:
//! - a key pressed since the last read counts as held (`isDown`: `wasDown() || bDown`);
//! - a key pressed since the last read counts as pressed once (`isDownOnce`: `nDownOnce`).
//!
//! So a tap shorter than the gap between two sub-steps is still seen.

/// The keys a player holds during a frame. Bits match `rl_oracle::Keys`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Keys(pub u8);

impl Keys {
    pub const UP: u8 = 1;
    pub const DOWN: u8 = 2;
    pub const LEFT: u8 = 4;
    pub const RIGHT: u8 = 8;
    pub const JUMP: u8 = 16;
    pub const FIRE: u8 = 32;
    pub const ROPE: u8 = 64;
    pub const CHANGE: u8 = 128;
}

/// One key's latched state between two input reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyLatch {
    down: bool,
    pressed: bool,
    once: bool,
}

impl KeyLatch {
    /// Sets the physical key state; a press (up to down) latches both flags.
    pub fn set(&mut self, down: bool) {
        if down && !self.down {
            self.pressed = true;
            self.once = true;
        }
        self.down = down;
    }

    /// `CInput::isDown`: down now, or pressed since the last read.
    pub fn is_down(&self) -> bool {
        self.down || self.pressed
    }

    /// `CInput::isDownOnce`: pressed since the last read.
    pub fn is_once(&self) -> bool {
        self.once
    }

    /// `CInput::reset`: called at the end of every read.
    pub fn reset(&mut self) {
        self.pressed = false;
        self.once = false;
    }
}

/// All keys of one worm.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Latches {
    pub up: KeyLatch,
    pub down: KeyLatch,
    pub left: KeyLatch,
    pub right: KeyLatch,
    pub jump: KeyLatch,
    pub fire: KeyLatch,
    pub rope: KeyLatch,
    pub change: KeyLatch,
}

impl Latches {
    /// Sets every key from this frame's `Keys`.
    pub fn set(&mut self, k: Keys) {
        let on = |bit: u8| k.0 & bit != 0;
        self.up.set(on(Keys::UP));
        self.down.set(on(Keys::DOWN));
        self.left.set(on(Keys::LEFT));
        self.right.set(on(Keys::RIGHT));
        self.jump.set(on(Keys::JUMP));
        self.fire.set(on(Keys::FIRE));
        self.rope.set(on(Keys::ROPE));
        self.change.set(on(Keys::CHANGE));
    }

    /// Ends a read: clears every key's latches.
    pub fn reset(&mut self) {
        for k in [
            &mut self.up,
            &mut self.down,
            &mut self.left,
            &mut self.right,
            &mut self.jump,
            &mut self.fire,
            &mut self.rope,
            &mut self.change,
        ] {
            k.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tap_is_seen_once_even_after_release() {
        let mut k = KeyLatch::default();
        k.set(true);
        k.set(false); // released before any read
        assert!(k.is_down() && k.is_once());
        k.reset();
        assert!(!k.is_down() && !k.is_once());
    }

    #[test]
    fn holding_is_down_but_once_only_after_a_press() {
        let mut k = KeyLatch::default();
        k.set(true);
        k.reset();
        k.set(true); // still held: no new press
        assert!(k.is_down() && !k.is_once());
    }
}
```

- [ ] **Step 6: Replace `xtask/layers.ron`**

```ron
// Crate layering (spec §4). A crate may depend on crates in lower layers, and on
// same-layer crates only when the pair is listed in `same_layer`. Every
// workspace crate must be listed here. `no_std` crates must declare
// #![no_std]; `wasm` crates must build for wasm32-unknown-unknown.
(
    layers: {
        "rl-math": 0,
        "rl-data": 1,
        "rl-terrain": 1,
        "rl-vfs": 1,
        "rl-pack": 2,
        "rl-import-lgs": 2,
        "rl-import-lxl": 2,
        "rl-import-gfx": 2,
        "rl-oracle": 2,
        "rl-sim": 3,
        "rl-cli": 6,
        "xtask": 6,
    },
    same_layer: [
        ("rl-terrain", "rl-data"),
        ("rl-import-lgs", "rl-pack"),
        ("rl-import-lxl", "rl-pack"),
        ("rl-import-gfx", "rl-pack"),
    ],
    no_std: ["rl-math", "rl-data", "rl-terrain", "rl-sim"],
    wasm: ["rl-math", "rl-data", "rl-terrain", "rl-sim", "rl-vfs", "rl-pack", "rl-import-lgs", "rl-import-lxl", "rl-import-gfx"],
)
```

- [ ] **Step 7: Run**

```bash
cargo nextest run -p rl-sim
```

Expected: output contains `4 passed`. `olx_header_gives_the_lx56_defaults`, unsupported-feature errors, and both latch rules.

- [ ] **Step 8: Run**

```bash
cargo xtask check --offline
```

Expected: output contains `check passed`.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "rl-sim: add settings and key latches"
```


---

### Task 5: The LX56 simulation, verified bit for bit against the oracle

Writes the acceptance test first (it fails: there is no `World` yet), then ports worm state, the ninja rope, worm physics with keyboard input, and the world's two clocks, until all seven oracle scenarios match OLX bit for bit.

**Files:**

- Create: `crates/rl-scenario/{Cargo.toml,CLAUDE.md}`, `crates/rl-scenario/src/lib.rs`, `crates/rl-scenario/tests/oracle.rs`
- Create: `crates/rl-sim/src/{worm,rope,physics,world}.rs`
- Modify: `crates/rl-sim/src/lib.rs` (all modules), `xtask/layers.ron` (register `rl-scenario`, layer 4)

**Interfaces:**

- Consumes: `SimSettings`, `Keys`, `Latches` (Task 4); `Terrain`, `Stamp` (Task 3); `Vec2`, `AbsTime`, `TimeDiff` and the macros (Task 2); `Scenario`, `Trace`, `compare` (plan 2).
- Produces: `World::new(SimSettings, Terrain, holes: Vec<Stamp> /* 5 */, start: AbsTime)`, `spawn_worm(x: i32, y: i32) -> usize`, `align_physics_clock()`, `tick(&[Keys])`, `worms()`, `worms_mut()`, `time()`, `pub terrain`, `pub settings`; `World: Clone + PartialEq`.
- Produces: `Worm { id, alive, pos, vel, health, angle, angle_speed, face: Dir, move_dir: Dir, on_ground, state: WormState, current_weapon, slots: Vec<Slot>, rope: NinjaRope, lives, kills, deaths, dirt_count, frame, speed_factor, can_use_ninja, can_air_jump, last_input_time, last_air_jump_time }`.
- Produces: `Dir { Left = 0, Right = 1 }`; `WormState { shoot, carve, moving, jump }` with `as_int()`; `Slot { weapon, charge, reloading, last_fire }`; `NinjaRope { released, shooting, attached, player_attached, pos, vel }`.
- Produces: `rl_scenario::{load_fixture, fixture_names, load_world, record, replay}`; `replay(content: &Path, &Scenario, expected: &Trace) -> anyhow::Result<Trace>`.

- [ ] **Step 1: Run**

```bash
cargo xtask fetch-olx-content
```

Expected: output contains `RL_CONTENT_DIR=`. The acceptance test needs the imported Liero Factory and Dirt Level packs and the Dirt theme.

- [ ] **Step 2: Create `crates/rl-scenario/Cargo.toml`**

```toml
[package]
name = "rl-scenario"
description = "Replays oracle scenarios in the simulation and records traces to compare with OLX."
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
rl-math.workspace = true
rl-data.workspace = true
rl-terrain.workspace = true
rl-sim.workspace = true
rl-oracle.workspace = true
rl-pack.workspace = true
rl-vfs.workspace = true
anyhow.workspace = true

[lints]
workspace = true
```

- [ ] **Step 3: Create `crates/rl-scenario/CLAUDE.md`**

```markdown
# rl-scenario

Plays oracle scenarios (`fixtures/oracle/*.ron`) in `rl-sim` and records
traces in `rl-oracle`'s format.

- **Layer 4, uses `std`.**
- The scenario's world comes from the imported packs in a content directory
  (`target/content`, written by `rl-cli import`) plus OLX's settings from the
  trace header.
- Weapon slots, lives and kills are copied from OLX's frame −1 until M4
  simulates weapon selection.
- `tests/oracle.rs` is M3's acceptance test: every scenario must match bit for bit.
- For debugging a divergence: `rl-cli scenario <name> --dump <frame>`.

Test: `cargo nextest run -p rl-scenario` (with `RL_CONTENT_DIR`; `cargo xtask check` sets it).
```

- [ ] **Step 4: Create `crates/rl-scenario/src/lib.rs`**

```rust
//! Plays oracle scenarios (`fixtures/oracle/*.ron`) in the Rust simulation and
//! records a [`Trace`] in the oracle's format, so `rl_oracle::compare` can
//! check it against OLX's recording.
//!
//! Weapon slots, lives and kills are copied from OLX's frame -1: weapon
//! selection is simulated only from M4 on.

use std::path::Path;

use anyhow::Context;
use rl_math::AbsTime;
use rl_oracle::{Frame, Num, RopeRec, Scenario, SlotRec, Trace, WormRec};
use rl_sim::{Keys, SimSettings, Slot, World};
use rl_terrain::{Stamp, Terrain};
use rl_vfs::DirFs;

/// Reads `<dir>/<name>.ron` and `<dir>/<name>.jsonl.gz`.
pub fn load_fixture(dir: &Path, name: &str) -> anyhow::Result<(Scenario, Trace)> {
    let ron = dir.join(format!("{name}.ron"));
    let sc = Scenario::from_ron(&std::fs::read_to_string(&ron).with_context(|| ron.display().to_string())?)?;
    let gz = dir.join(format!("{name}.jsonl.gz"));
    let trace = Trace::from_gz(&std::fs::read(&gz).with_context(|| gz.display().to_string())?)?;
    Ok((sc, trace))
}

/// Scenario names in `dir`, sorted.
pub fn fixture_names(dir: &Path) -> anyhow::Result<Vec<String>> {
    let mut names = Vec::new();
    for e in std::fs::read_dir(dir).with_context(|| dir.display().to_string())? {
        let p = e?.path();
        if p.extension().is_some_and(|x| x == "ron") {
            names.push(p.file_stem().expect("file name").to_string_lossy().into_owned());
        }
    }
    names.sort();
    Ok(names)
}

/// The world a scenario starts from: the imported level and theme from
/// `content` (packs written by `rl-cli import`), OLX's settings from the trace
/// header, and the scenario's worms spawned.
pub fn load_world(content: &Path, sc: &Scenario, expected: &Trace) -> anyhow::Result<World> {
    let stem = sc.level.rsplit_once('.').map_or(sc.level.as_str(), |(s, _)| s);
    let level_id = format!("olx-level-{}", rl_data::slug::slug(stem));
    let level = rl_pack::read_level(&DirFs::new(content.join(&level_id)))
        .with_context(|| format!("level pack {level_id}"))?
        .level;
    let theme = rl_pack::read_theme(&DirFs::new(content.join(&level.def.theme)))
        .with_context(|| format!("theme pack {}", level.def.theme))?
        .theme;
    let settings =
        SimSettings::from_olx_features(&expected.header.features).map_err(|e| anyhow::anyhow!(e.0))?;
    let holes = theme.holes.iter().map(Stamp::from_image).collect();
    let mut world = World::new(settings, Terrain::from_grid(&level.material), holes, AbsTime(0));
    let start = expected.frames.first().context("trace has no frames")?;
    for (i, spawn) in sc.worms.iter().enumerate() {
        world.spawn_worm(spawn.x, spawn.y);
        let rec = start.w.get(i).context("trace has fewer worms than the scenario")?;
        let w = &mut world.worms_mut()[i];
        w.slots = rec
            .slots
            .iter()
            .map(|s| Slot { weapon: s.id, charge: s.ch.0, reloading: s.rl, last_fire: s.lf.0 })
            .collect();
        w.current_weapon = rec.wpn;
        (w.lives, w.kills, w.deaths) = (rec.lives, rec.kills, rec.deaths);
    }
    Ok(world)
}

/// The world's state as a trace frame.
pub fn record(world: &World, f: i64) -> Frame {
    let w = world
        .worms()
        .iter()
        .map(|w| WormRec {
            id: w.id,
            alive: w.alive,
            x: Num(w.pos.x),
            y: Num(w.pos.y),
            vx: Num(w.vel.x),
            vy: Num(w.vel.y),
            hp: Num(w.health),
            ang: Num(w.angle),
            angv: Num(w.angle_speed),
            face: w.face as i32,
            move_dir: w.move_dir as i32,
            ground: w.on_ground,
            keys: w.state.as_int() as u32,
            wpn: w.current_weapon,
            slots: w
                .slots
                .iter()
                .map(|s| SlotRec { id: s.weapon, ch: Num(s.charge), rl: s.reloading, lf: Num(s.last_fire) })
                .collect(),
            rope: RopeRec {
                rel: w.rope.released,
                sh: w.rope.shooting,
                at: w.rope.attached,
                pw: w.rope.player_attached,
                x: Num(w.rope.pos.x),
                y: Num(w.rope.pos.y),
                vx: Num(w.rope.vel.x),
                vy: Num(w.rope.vel.y),
            },
            lives: w.lives,
            kills: w.kills,
            deaths: w.deaths,
            dirt: w.dirt_count,
        })
        .collect();
    Frame { f, w, p: vec![], b: vec![], mh: format!("{:016x}", world.terrain.material_hash()) }
}

/// Plays `sc` from frame -1 to its last frame and returns the Rust trace,
/// with the same header as `expected`.
pub fn replay(content: &Path, sc: &Scenario, expected: &Trace) -> anyhow::Result<Trace> {
    let mut world = load_world(content, sc, expected)?;
    let mut frames = vec![record(&world, -1)];
    for (f, keys) in sc.keys_per_frame().into_iter().enumerate() {
        let keys: Vec<Keys> = keys.iter().map(|k| Keys(k.0)).collect();
        world.tick(&keys);
        frames.push(record(&world, f as i64));
    }
    Ok(Trace { header: expected.header.clone(), frames })
}
```

- [ ] **Step 5: Create `crates/rl-scenario/tests/oracle.rs`**

```rust
//! M3 acceptance (spec §6, §10): every scenario in `fixtures/oracle`, replayed
//! in the Rust simulation, matches OLX's recorded trace bit for bit.
//! Needs the imported OLX content; `cargo xtask check` sets `RL_CONTENT_DIR`.

use std::path::{Path, PathBuf};

#[test]
fn every_scenario_matches_olx_bit_for_bit() {
    let Ok(content) = std::env::var("RL_CONTENT_DIR") else {
        eprintln!("RL_CONTENT_DIR not set; skipping the oracle comparison");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/oracle");
    let names = rl_scenario::fixture_names(&dir).unwrap();
    assert_eq!(names, ["aim", "dig", "fall", "jump", "rope", "tap", "walk"]);
    let mut failures = Vec::new();
    for name in names {
        let (sc, expected) = rl_scenario::load_fixture(&dir, &name).unwrap();
        let actual = rl_scenario::replay(Path::new(&content), &sc, &expected).unwrap();
        if let Err(d) = rl_oracle::compare(&expected, &actual, &sc.tolerances) {
            failures.push(format!("{name}: {d}"));
        }
    }
    assert!(failures.is_empty(), "diverged from OLX:\n{}", failures.join("\n"));
}

#[test]
fn a_changed_trace_is_detected() {
    let Ok(content) = std::env::var("RL_CONTENT_DIR") else { return };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/oracle");
    let (sc, mut expected) = rl_scenario::load_fixture(&dir, "walk").unwrap();
    let actual = rl_scenario::replay(Path::new(&content), &sc, &expected).unwrap();
    expected.frames[200].w[0].x = rl_oracle::Num(expected.frames[200].w[0].x.0 + 1.0);
    let d = rl_oracle::compare(&expected, &actual, &[]).unwrap_err();
    assert_eq!((d.frame, d.path.as_str()), (199, "w[0].x"));
}
```

- [ ] **Step 6: Replace `xtask/layers.ron`**

```ron
// Crate layering (spec §4). A crate may depend on crates in lower layers, and on
// same-layer crates only when the pair is listed in `same_layer`. Every
// workspace crate must be listed here. `no_std` crates must declare
// #![no_std]; `wasm` crates must build for wasm32-unknown-unknown.
(
    layers: {
        "rl-math": 0,
        "rl-data": 1,
        "rl-terrain": 1,
        "rl-vfs": 1,
        "rl-pack": 2,
        "rl-import-lgs": 2,
        "rl-import-lxl": 2,
        "rl-import-gfx": 2,
        "rl-oracle": 2,
        "rl-sim": 3,
        "rl-scenario": 4,
        "rl-cli": 6,
        "xtask": 6,
    },
    same_layer: [
        ("rl-terrain", "rl-data"),
        ("rl-import-lgs", "rl-pack"),
        ("rl-import-lxl", "rl-pack"),
        ("rl-import-gfx", "rl-pack"),
    ],
    no_std: ["rl-math", "rl-data", "rl-terrain", "rl-sim"],
    wasm: ["rl-math", "rl-data", "rl-terrain", "rl-sim", "rl-vfs", "rl-pack", "rl-import-lgs", "rl-import-lxl", "rl-import-gfx"],
)
```

- [ ] **Step 7: Run**

```bash
cargo nextest run -p rl-scenario
```

 Expected to FAIL to compile with "unresolved imports `rl_sim::Slot`, `rl_sim::World`": the simulation does not exist yet.

- [ ] **Step 8: Create `crates/rl-sim/src/worm.rs`**

```rust
//! Worm state (OLX `CWorm`, `src/game/CWorm.h`), only what the simulation uses.

use alloc::vec::Vec;

use rl_math::{AbsTime, PI_F32, Vec2};

use crate::rope::NinjaRope;

/// OLX `DIR_TYPE`: left is 0, right is 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left = 0,
    Right = 1,
}

/// OLX `worm_state_t`: what the worm does this sub-step, derived from input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WormState {
    pub shoot: bool,
    pub carve: bool,
    pub moving: bool,
    pub jump: bool,
}

impl WormState {
    /// `worm_state_t::asInt` (`src/common/CWorm.cpp:62-69`): shoot 1, carve 2, move 4, jump 8.
    pub fn as_int(&self) -> u8 {
        self.shoot as u8 | (self.carve as u8) << 1 | (self.moving as u8) << 2 | (self.jump as u8) << 3
    }
}

/// OLX `wpnslot_t`. Not simulated in M3: weapons arrive with M4.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub weapon: i32,
    pub charge: f32,
    pub reloading: bool,
    pub last_fire: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Worm {
    pub id: i32,
    pub alive: bool,
    pub pos: Vec2,
    pub vel: Vec2,
    pub health: f32,
    /// Aim angle in degrees: negative is up.
    pub angle: f32,
    pub angle_speed: f32,
    pub face: Dir,
    pub move_dir: Dir,
    pub on_ground: bool,
    pub state: WormState,
    pub current_weapon: i32,
    pub slots: Vec<Slot>,
    pub rope: NinjaRope,
    pub lives: i32,
    pub kills: i32,
    pub deaths: i32,
    pub dirt_count: i32,
    /// Animation frame (0..3), only for rendering.
    pub frame: f32,
    pub speed_factor: f32,
    pub can_use_ninja: bool,
    pub can_air_jump: bool,
    pub last_input_time: AbsTime,
    pub last_air_jump_time: AbsTime,
}

impl Worm {
    /// A worm as `CWorm::Spawn` leaves it (`src/common/CWorm.cpp:591-650`),
    /// except the carving, which [`crate::World::spawn_worm`] does.
    pub fn spawned(id: i32, pos: Vec2, now: AbsTime, speed_factor: f32) -> Self {
        Self {
            id,
            alive: true,
            pos,
            vel: Vec2::ZERO,
            health: 100.0,
            angle: 0.0,
            angle_speed: 0.0,
            face: Dir::Right,
            move_dir: Dir::Right,
            on_ground: false,
            state: WormState::default(),
            current_weapon: 0,
            slots: Vec::new(),
            rope: NinjaRope::default(),
            lives: -2,
            kills: 0,
            deaths: 0,
            dirt_count: 0,
            frame: 0.0,
            speed_factor,
            can_use_ninja: true,
            can_air_jump: false,
            last_input_time: now,
            last_air_jump_time: AbsTime(0),
        }
    }

    fn direction(&self, side: Dir) -> Vec2 {
        let rad = self.angle * (PI_F32 / 180.0);
        let sx = if side == Dir::Left { -1.0 } else { 1.0 };
        Vec2::new(libm::cosf(rad) * sx, libm::sinf(rad))
    }

    /// `CWorm::getFaceDirection` (`src/game/CWorm.h:514-516`).
    pub fn face_direction(&self) -> Vec2 {
        self.direction(self.face)
    }

    /// `CWorm::getMoveDirection` (`src/game/CWorm.h:518-520`).
    pub fn move_direction(&self) -> Vec2 {
        self.direction(self.move_dir)
    }
}
```

- [ ] **Step 9: Create `crates/rl-sim/src/rope.rs`**

```rust
//! The ninja rope (OLX `CNinjaRope`, `src/client/CNinjaRope.cpp`, and
//! `simulateNinjarope`, `src/common/PhysicsLX56.cpp:505-604`).

use rl_data::level::Material;
use rl_math::Vec2;
use rl_terrain::Terrain;

use crate::SimSettings;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NinjaRope {
    pub released: bool,
    pub shooting: bool,
    pub attached: bool,
    /// Id of the worm the hook is attached to, or -1.
    pub player_attached: i32,
    /// Hook position and velocity.
    pub pos: Vec2,
    pub vel: Vec2,
}

impl Default for NinjaRope {
    /// A cleared rope (`CNinjaRope::Clear`): nothing attached, player -1.
    fn default() -> Self {
        Self {
            released: false,
            shooting: false,
            attached: false,
            player_attached: -1,
            pos: Vec2::ZERO,
            vel: Vec2::ZERO,
        }
    }
}

impl NinjaRope {
    /// `CNinjaRope::Clear`.
    pub fn clear(&mut self) {
        self.released = false;
        self.shooting = false;
        self.attached = false;
        self.player_attached = -1;
    }

    /// `CNinjaRope::Shoot` from `owner_pos` towards `dir`.
    pub fn shoot(&mut self, owner_pos: Vec2, owner_vel: Vec2, dir: Vec2, s: &SimSettings) {
        self.clear();
        self.released = true;
        self.shooting = true;
        self.pos = owner_pos;
        self.vel = dir * s.rope_speed;
        if s.rope_add_parent_speed {
            self.vel += owner_vel;
        }
    }

    /// `CNinjaRope::Attach`.
    fn attach(&mut self) {
        self.shooting = false;
        self.attached = true;
        self.vel = Vec2::ZERO;
    }

    /// `CNinjaRope::GetForce`: the pull on the owner at `owner_pos`.
    pub fn force(&self, owner_pos: Vec2, s: &SimSettings) -> Vec2 {
        if !self.attached {
            return Vec2::ZERO;
        }
        let dir = (owner_pos - self.pos).normalize();
        let rest = s.rope_rest_length as f32;
        if (owner_pos - self.pos).length2() < rest * rest {
            return Vec2::ZERO;
        }
        dir * (s.rope_strength * -100.0)
    }

    /// `simulateNinjarope` for one sub-step of `dt` seconds. `owner_pos` is the
    /// owner's position at the start of the step; `worms` is `(id, alive, pos)`
    /// of every worm, for hooking onto other worms.
    pub fn simulate(
        &mut self,
        dt: f32,
        owner_id: i32,
        owner_pos: Vec2,
        worms: &[(i32, bool, Vec2)],
        terrain: &Terrain,
        s: &SimSettings,
    ) {
        if !self.released {
            return;
        }
        let first_hit = !self.attached;
        let mut force = Vec2::new(0.0, if self.shooting { s.rope_gravity } else { s.rope_falling_gravity });

        if (self.vel + force * dt).length2() * dt * dt * s.ninjarope_precision > 5.0 {
            self.simulate(dt / 2.0, owner_id, owner_pos, worms, terrain, s);
            self.simulate(dt / 2.0, owner_id, owner_pos, worms, terrain, s);
            return;
        }

        if self.shooting {
            self.vel += force * dt;
            self.pos += self.vel * dt;
            let length2 = (owner_pos - self.pos).length2();
            let max = s.rope_max_length as f32;
            if length2 > max * max {
                self.vel = Vec2::ZERO;
                self.shooting = false;
            }
        } else if !self.attached {
            let length2 = (owner_pos - self.pos).length2();
            let rest = s.rope_rest_length as f32;
            if length2 > rest * rest {
                let mut d = owner_pos - self.pos;
                if length2 != 0.0 {
                    d *= 1.0 / libm::sqrtf(length2);
                }
                force += (d * 10000.0) * dt;
            }
            self.vel += force * dt;
            self.pos += self.vel * dt;
        }

        let player_attached = self.player_attached_alive(worms);
        let mut outside_map = false;
        let (w, h) = (terrain.width() as f32, terrain.height() as f32);
        if !player_attached
            && (self.pos.x <= 0.0 || self.pos.y <= 0.0 || self.pos.x >= w - 1.0 || self.pos.y >= h - 1.0)
        {
            self.attach();
            self.pos.x = if 0.0 > self.pos.x { 0.0 } else { self.pos.x };
            self.pos.y = if 0.0 > self.pos.y { 0.0 } else { self.pos.y };
            self.pos.x = if w - 1.0 < self.pos.x { w - 1.0 } else { self.pos.x };
            self.pos.y = if h - 1.0 < self.pos.y { h - 1.0 } else { self.pos.y };
            outside_map = true;
        }

        if !player_attached {
            self.attached = false;
            let wx = (self.pos.x as i64).rem_euclid(terrain.width() as i64);
            let wy = (self.pos.y as i64).rem_euclid(terrain.height() as i64);
            let m = terrain.material(wx, wy).unwrap_or(Material::Rock);
            // Rock and dirt stop particles and can be hooked; background lets them pass.
            let particle_pass = m == Material::Background;
            let can_hook = m != Material::Background;
            if !particle_pass || outside_map {
                if !outside_map && !can_hook {
                    self.vel = Vec2::ZERO;
                    self.shooting = false;
                } else {
                    self.attach();
                    // OLX spawns cosmetic dirt particles here on the first hit (rand, M4).
                    let _ = first_hit;
                }
            }
        }

        self.check_worm_attachment(owner_id, worms, s);
    }

    fn player_attached_alive(&self, worms: &[(i32, bool, Vec2)]) -> bool {
        self.player_attached >= 0 && worms.iter().any(|(id, _, _)| *id == self.player_attached)
    }

    /// `CNinjaRope::checkForWormAttachment`.
    fn check_worm_attachment(&mut self, owner_id: i32, worms: &[(i32, bool, Vec2)], s: &SimSettings) {
        if s.rope_can_attach_worm && !self.attached && !self.player_attached_alive(worms) {
            for &(id, alive, pos) in worms {
                if id == owner_id || !alive {
                    continue;
                }
                if (pos - self.pos).length2() < 25.0 {
                    self.attach();
                    self.player_attached = id;
                    break;
                }
            }
        } else if self.attached && self.player_attached_alive(worms) {
            match worms.iter().find(|(id, _, _)| *id == self.player_attached) {
                Some(&(_, true, pos)) => self.pos = pos,
                _ => {
                    // UnAttachPlayer
                    self.vel = Vec2::ZERO;
                    self.shooting = false;
                    self.attached = false;
                    self.player_attached = -1;
                }
            }
        }
    }
}
```

- [ ] **Step 10: Create `crates/rl-sim/src/physics.rs`**

```rust
//! LX56 worm simulation (`src/common/PhysicsLX56.cpp:88-469`) and the human
//! keyboard input it reads (`CWormHumanInputHandler::getInput`,
//! `src/common/CWormHuman.cpp:81-560`). Every expression keeps OLX's
//! operation order: results must match the oracle bit for bit.

use rl_math::{AbsTime, TimeDiff, Vec2, clamp_direct, reduce_const, reset_small, sign};
use rl_terrain::{PX_EMPTY, Stamp, Terrain};

use crate::SimSettings;
use crate::input::Latches;
use crate::worm::{Dir, Worm};

/// Everything one worm's sub-step needs besides the worm itself.
pub(crate) struct Ctx<'a> {
    pub now: AbsTime,
    pub dt: f32,
    pub terrain: &'a mut Terrain,
    pub holes: &'a [Stamp],
    pub settings: &'a SimSettings,
    /// `(id, alive, pos)` of every worm, for the rope.
    pub worms: &'a [(i32, bool, Vec2)],
}

/// LX56 aim (`ForceLX56Aim`): max speed, acceleration.
const AIM_MAX_SPEED: f32 = 100.0;
const AIM_ACCEL: f32 = 500.0;

/// The keyboard part of `CWormHumanInputHandler::getInput` with OLX's default
/// client options (no mouse aim, no strafing, no old-school rope).
pub(crate) fn get_input(w: &mut Worm, keys: &mut Latches, now: AbsTime, s: &SimSettings) {
    let jump = keys.jump.is_once();
    let left_once = keys.left.is_once();
    let right_once = keys.right.is_once();

    if !w.alive {
        keys.reset();
        return;
    }

    let dt = if now > w.last_input_time {
        let d = now - w.last_input_time;
        w.last_input_time = now;
        d
    } else {
        TimeDiff(0)
    };

    w.state.carve = false;
    w.state.moving = false;
    w.state.shoot = false;
    w.state.jump = false;

    // Aim, LX56 style: no friction, a constant slow-down when no key is held.
    if keys.up.is_down() {
        w.angle_speed -= AIM_ACCEL * dt.seconds();
    } else if keys.down.is_down() {
        w.angle_speed += AIM_ACCEL * dt.seconds();
    } else {
        clamp_direct(&mut w.angle_speed, -AIM_MAX_SPEED, AIM_MAX_SPEED);
        reduce_const(&mut w.angle_speed, 200.0 * dt.seconds());
        reset_small(&mut w.angle_speed, 5.0);
    }
    w.angle += w.angle_speed * dt.seconds();
    let max_angle = if s.full_aim_angle { 90.0 } else { 60.0 };
    if clamp_direct(&mut w.angle, -90.0, max_angle) != 0 {
        w.angle_speed = 0.0;
    }

    // Taken before this read changes the facing direction.
    let ninja_dir = w.face_direction();

    w.state.shoot = keys.fire.is_down();
    let slots = w.slots.len() as i32;
    if slots > 0 && !w.state.shoot && keys.change.is_down() {
        let change = right_once as i32 - left_once as i32;
        w.current_weapon = (w.current_weapon + change).rem_euclid(slots);
    }

    if !keys.change.is_down() {
        if keys.left.is_down() {
            w.state.moving = true;
            if !keys.right.is_down() {
                w.face = Dir::Left;
                w.move_dir = Dir::Left;
            }
            if right_once {
                w.state.carve = true;
            }
        }
        if keys.right.is_down() {
            w.state.moving = true;
            if !keys.left.is_down() {
                w.face = Dir::Right;
                w.move_dir = Dir::Right;
            }
            if left_once {
                w.state.carve = true;
            }
        }
    }

    w.state.jump |= jump;
    if jump && w.rope.released {
        w.rope.clear();
    }
    if keys.rope.is_once() && w.can_use_ninja {
        let (pos, vel) = (w.pos, w.vel);
        w.rope.shoot(pos, vel, ninja_dir, s);
    }

    keys.reset();
}

/// `CWorm::CheckOnGround` (`src/common/CWorm.cpp:1363-1379`).
fn check_on_ground(w: &Worm, t: &Terrain) -> bool {
    let (px, py) = (w.pos.x as i32 as i64, w.pos.y as i32 as i64);
    (1..=6).rev().any(|y| t.flags(px - 2, py + y) & PX_EMPTY == 0 || t.flags(px + 2, py + y) & PX_EMPTY == 0)
}

/// The one-argument `CarveHole(CVec)` (`src/common/CMap.cpp:2172-2199`): a size-3 hole.
/// OLX also throws cosmetic dirt particles here (consuming `rand()`, M4).
fn carve_small(ctx: &mut Ctx, pos: Vec2) -> i32 {
    ctx.terrain.carve_hole(&ctx.holes[3], pos.x as i32, pos.y as i32) as i32
}

/// `PhysicsEngine::simulateWorm` for a local worm (`PhysicsLX56.cpp:272-469`),
/// without the weapon part (M4). Input is read first, as in OLX.
pub(crate) fn simulate_worm(w: &mut Worm, keys: &mut Latches, ctx: &mut Ctx) {
    let s = ctx.settings;
    get_input(w, keys, ctx.now, s);
    if !w.alive {
        return;
    }
    let dt = ctx.dt;

    if w.state.carve {
        let dir = w.move_direction();
        let carved = carve_small(ctx, w.pos + dir * 4.0);
        w.dirt_count += carved;
    }

    if w.state.moving {
        w.frame += 7.5 * dt;
    }
    if w.frame >= 3.0 || !w.state.moving {
        w.frame = 0.0;
    }
    if w.frame < 0.0 {
        w.frame = 2.99;
    }

    if w.rope.released {
        let (id, pos) = (w.id, w.pos);
        w.rope.simulate(dt, id, pos, ctx.worms, ctx.terrain, s);
        w.vel += w.rope.force(w.pos, s) * dt;
    }

    if w.state.moving {
        let (speed, max) = if w.on_ground {
            (s.worm_ground_speed, s.worm_max_ground_move_speed)
        } else {
            (s.worm_air_speed, s.worm_max_air_move_speed)
        };
        if w.move_dir == Dir::Right {
            if w.vel.x < max {
                w.vel.x += speed * dt * 90.0;
            }
        } else if w.vel.x > -max {
            w.vel.x -= speed * dt * 90.0;
        }
    }

    let mut jumped = false;
    let on_ground = check_on_ground(w, ctx.terrain);
    if on_ground {
        w.last_air_jump_time = AbsTime(0);
    }
    let relative_ok = s.relative_air_jump
        && ctx.now > AbsTime(w.last_air_jump_time.0 + (s.relative_air_jump_delay * 1000.0) as u64);
    if w.state.jump && (on_ground || w.can_air_jump || relative_ok) {
        // Ground jump, or OLX's "instant air jump" (absolute velocity); both set vy.
        if on_ground || (!s.jump_to_aim_dir && w.can_air_jump && w.vel.y > s.worm_jump_force) {
            w.vel.y = s.worm_jump_force;
        } else {
            let dir = if s.jump_to_aim_dir { -w.face_direction() } else { Vec2::new(0.0, 1.0) };
            w.vel += dir * s.worm_jump_force;
        }
        w.last_air_jump_time = ctx.now;
        w.on_ground = false;
        jumped = true;
    }

    if !w.on_ground {
        let drag = s.worm_air_friction;
        w.vel.x -= w.vel.x * w.vel.x * sign(w.vel.x) as f32 * drag * dt;
        w.vel.y += -(w.vel.y * w.vel.y) * sign(w.vel.y) as f32 * drag * dt;
    }

    w.vel.y += s.worm_gravity * dt;

    let mut vel = w.vel;
    let pos = w.pos;
    move_and_check_collision(dt, w, pos, &mut vel, pos, jumped, ctx.terrain);
    w.vel = vel;

    if w.on_ground {
        w.vel.x *= 1.0 - s.worm_ground_friction;
        if libm::fabsf(w.vel.x) < s.worm_ground_stop_speed && !w.state.moving {
            w.vel.x = 0.0;
        }
    }
}

/// `moveAndCheckWormCollision` (`PhysicsLX56.cpp:88-269`) without wrap-around.
/// It writes `w.pos` and `w.on_ground` directly and updates `vel`.
fn move_and_check_collision(
    mut dt: f32,
    w: &mut Worm,
    mut pos: Vec2,
    vel: &mut Vec2,
    old_pos: Vec2,
    jump: bool,
    t: &Terrain,
) -> bool {
    const MAX_SPEED2: f32 = 10.0;
    let (map_w, map_h) = (t.width(), t.height());

    if (*vel * dt * w.speed_factor).length2() > map_w as f32 * map_h as f32 {
        return true;
    }
    if (*vel * dt * w.speed_factor).length2() > MAX_SPEED2 && dt > 0.001 {
        dt /= 2.0;
        if move_and_check_collision(dt, w, pos, vel, old_pos, jump, t) {
            return true;
        }
        let pos2 = w.pos;
        return move_and_check_collision(dt, w, pos2, vel, old_pos, jump, t);
    }

    pos += *vel * dt * w.speed_factor;
    w.pos = pos;

    let mut clip = 0u8; // 1 left, 2 right, 4 top, 8 bottom
    let mut coll = false;
    let y = pos.y as i32;
    if y >= 0 && (y as u32) < map_h {
        for x in -3..4 {
            if pos.x + x as f32 <= 2.0 {
                clip |= 0x01;
                w.pos.x = 5.0;
                coll = true;
                bounce(&mut vel.x);
                continue;
            }
            if pos.x + x as f32 >= map_w as f32 {
                w.pos.x = map_w as f32 - 5.0;
                coll = true;
                clip |= 0x02;
                bounce(&mut vel.x);
                continue;
            }
            let posx = pos.x as i32 + x;
            if t.flags(posx as i64, y as i64) & PX_EMPTY == 0 {
                coll = true;
                if x < 0 {
                    clip |= 0x01;
                    w.pos.x = pos.x + x as f32 + 4.0;
                } else {
                    clip |= 0x02;
                    w.pos.x = pos.x + x as f32 - 4.0;
                }
                bounce(&mut vel.x);
            }
        }
    }

    w.on_ground = false;
    let mut hit = false;
    let x = pos.x as i32;
    if x >= 0 && (x as u32) < map_w {
        let mut y = 5;
        while y > -5 {
            if pos.y + y as f32 <= 1.0 {
                w.pos.y = 6.0;
                coll = true;
                clip |= 0x04;
                bounce(&mut vel.y);
                break;
            }
            if pos.y + y as f32 >= map_h as f32 {
                w.pos.y = map_h as f32 - 5.0;
                clip |= 0x08;
                coll = true;
                w.on_ground = true;
                bounce(&mut vel.y);
                y -= 1;
                continue;
            }
            let posy = pos.y as i32 + y;
            if t.flags(x as i64, posy as i64) & PX_EMPTY == 0 {
                coll = true;
                if !hit && !jump {
                    if libm::fabsf(vel.y) > 40.0 && ((vel.y > 0.0 && y > 0) || (vel.y < 0.0 && y < 0)) {
                        vel.y *= -0.4;
                    } else {
                        vel.y = 0.0;
                    }
                }
                hit = true;
                w.on_ground = true;
                if y < 0 {
                    clip |= 0x04;
                    w.pos.y = pos.y + y as f32 + 5.0;
                } else {
                    clip |= 0x08;
                    w.pos.y = pos.y + y as f32 - 5.0;
                }
            }
            y -= 1;
        }
    }

    if clip & 0x01 != 0 && clip & 0x02 != 0 {
        w.pos.x = old_pos.x;
    }
    if clip & 0x04 != 0 && clip & 0x08 != 0 {
        w.pos.y = old_pos.y;
        if !w.state.jump {
            w.pos.x = old_pos.x;
        }
    }
    coll
}

/// OLX's wall bounce: fast hits rebound at 40 %, slow ones stop.
fn bounce(v: &mut f32) {
    if libm::fabsf(*v) > 40.0 {
        *v *= -0.4;
    } else {
        *v = 0.0;
    }
}
```

- [ ] **Step 11: Create `crates/rl-sim/src/world.rs`**

```rust
//! The world and its two clocks (OLX `Game::frameInner`, `src/game/Game.cpp:897-959`,
//! and `CClient::Simulation`, `src/client/CClient_Game.cpp:60-222`).
//!
//! Each [`World::tick`] is one 100 Hz frame: it latches the frame's keys, runs
//! as many LX56 physics sub-steps (`1000 / LX56PhysicsFPS` whole milliseconds,
//! 11 ms by default) as fit up to the current frame time, then advances the
//! frame clock by 10 ms. Some frames run no sub-step at all.

use alloc::vec::Vec;

use rl_math::{AbsTime, TimeDiff, Vec2};
use rl_terrain::{Stamp, Terrain};

use crate::SimSettings;
use crate::input::{Keys, Latches};
use crate::physics::{Ctx, simulate_worm};
use crate::worm::Worm;

/// OLX `SPAWN_HOLESIZE` (`include/Consts.h:36`): the size-4 (largest) hole.
const SPAWN_HOLE: usize = 4;
/// `Game::FixedFrameTime`.
const FRAME_MS: u64 = 10;

#[derive(Clone, Debug, PartialEq)]
pub struct World {
    pub settings: SimSettings,
    pub terrain: Terrain,
    /// The theme's five hole stamps, smallest first.
    holes: Vec<Stamp>,
    worms: Vec<Worm>,
    latches: Vec<Latches>,
    /// `Game::simulationTime`.
    time: AbsTime,
    /// `CClient::fLastSimulationTime`: where the physics clock is.
    physics_time: AbsTime,
}

impl World {
    /// `holes` must be the theme's five hole stamps (`hole1` to `hole5`).
    pub fn new(settings: SimSettings, terrain: Terrain, holes: Vec<Stamp>, start: AbsTime) -> Self {
        assert_eq!(holes.len(), 5, "a theme has five hole stamps");
        Self {
            settings,
            terrain,
            holes,
            worms: Vec::new(),
            latches: Vec::new(),
            time: start,
            physics_time: start,
        }
    }

    pub fn worms(&self) -> &[Worm] {
        &self.worms
    }

    pub fn worms_mut(&mut self) -> &mut [Worm] {
        &mut self.worms
    }

    pub fn time(&self) -> AbsTime {
        self.time
    }

    /// `CWorm::Spawn` at `(x, y)`, including the spawn hole. Returns the worm's index.
    pub fn spawn_worm(&mut self, x: i32, y: i32) -> usize {
        let id = self.worms.len() as i32;
        let pos = Vec2::new(x as f32, y as f32);
        self.worms.push(Worm::spawned(id, pos, self.time, self.settings.worm_speed_factor));
        self.latches.push(Latches::default());
        self.terrain.carve_hole(&self.holes[SPAWN_HOLE], pos.x as i32, pos.y as i32);
        self.worms.len() - 1
    }

    /// Puts the physics clock at the frame clock, as OLX does when a match or
    /// an oracle scenario starts (`OracleHooks::loadInputs`, `simStep`).
    pub fn align_physics_clock(&mut self) {
        self.physics_time = self.time;
    }

    fn physics_dt(&self) -> TimeDiff {
        TimeDiff((1000 / self.settings.lx56_physics_fps.max(1)) as u64)
    }

    /// One 100 Hz frame. `keys[i]` is what worm `i` holds during this frame.
    pub fn tick(&mut self, keys: &[Keys]) {
        self.gus_speed_round_trip();
        for (latch, k) in self.latches.iter_mut().zip(keys) {
            latch.set(*k);
        }
        let speed = self.settings.game_speed.clamp(0.05, 10.0);
        let step = self.physics_dt();
        let mut step_clock = step;
        let mut frame = TimeDiff(FRAME_MS);
        if !self.settings.game_speed_only_for_projs {
            step_clock = step.scaled(1.0 / speed);
            if step_clock.0 == 0 {
                step_clock = TimeDiff(1);
            }
            frame = frame.scaled(1.0 / speed);
            if frame.0 == 0 {
                frame = TimeDiff(1);
            }
        }
        while self.physics_time + step_clock <= self.time {
            self.physics_step(step.seconds());
            self.physics_time += step_clock;
        }
        self.time += frame;
    }

    /// OLX runs `gusLogicFrame()` every frame, even for classic mods, inside a
    /// `GusSpeedScope` (`src/game/Game.cpp:682-691`, `945-949`) that converts
    /// every worm's and rope's velocity to Gusanos units and back
    /// (`CGameObject::ScopedGusCompatibleSpeed`, `src/common/CGameObject.cpp:105-127`):
    /// `v *= GusDT.seconds()` then `v *= 1.0f / GusDT.seconds()`, with `GusDT` = 10 ms.
    /// The round trip is lossy, and the oracle shows its effect, so it is kept.
    fn gus_speed_round_trip(&mut self) {
        let to_gus = TimeDiff(FRAME_MS).seconds();
        let from_gus = 1.0 / to_gus;
        for w in &mut self.worms {
            w.vel *= to_gus;
            w.vel *= from_gus;
            w.rope.vel *= to_gus;
            w.rope.vel *= from_gus;
        }
    }

    fn physics_step(&mut self, dt: f32) {
        for i in 0..self.worms.len() {
            let others: Vec<(i32, bool, Vec2)> = self.worms.iter().map(|w| (w.id, w.alive, w.pos)).collect();
            let mut ctx = Ctx {
                now: self.time,
                dt,
                terrain: &mut self.terrain,
                holes: &self.holes,
                settings: &self.settings,
                worms: &others,
            };
            simulate_worm(&mut self.worms[i], &mut self.latches[i], &mut ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use rl_data::image::{Rgba, RgbaImage};
    use rl_data::level::{Material, MaterialGrid};

    fn holes() -> Vec<Stamp> {
        // Same sizes as the Dirt theme's hole1..hole5.
        [5, 7, 9, 11, 15]
            .into_iter()
            .map(|size| {
                let mut img = RgbaImage::new(size, size);
                for y in 0..size {
                    for x in 0..size {
                        img.set(x, y, Rgba::MAGENTA);
                    }
                }
                Stamp::from_image(&img)
            })
            .collect()
    }

    fn world(fill: Material) -> World {
        let terrain = Terrain::from_grid(&MaterialGrid::new(200, 200, fill));
        World::new(SimSettings::lx56(), terrain, holes(), AbsTime(0))
    }

    #[test]
    fn physics_steps_follow_the_two_clocks() {
        // 11 ms physics steps against 10 ms frames: none in frames 0 and 1,
        // one per frame from frame 2, none again in frame 12 (OLX trace `fall`).
        let mut w = world(Material::Background);
        w.spawn_worm(100, 20);
        let mut stepped = vec![];
        for _ in 0..13 {
            let before = w.worms()[0].vel.y;
            w.tick(&[Keys(0)]);
            stepped.push(w.worms()[0].vel.y - before > 0.5);
        }
        let expected = [false, false, true, true, true, true, true, true, true, true, true, true, false];
        assert_eq!(stepped, expected);
    }

    #[test]
    fn gus_speed_round_trip_matches_olx() {
        // From the oracle: OLX turns 6.60000134 into 6.60000086 between frames 7 and 8 of `fall`.
        let mut w = world(Material::Background);
        w.spawn_worm(100, 20);
        w.worms_mut()[0].vel.y = f32::from_bits(0x40d3_3336);
        w.tick(&[Keys(0)]); // frame 0 runs no physics step
        assert_eq!(w.worms()[0].vel.y.to_bits(), 0x40d3_3335);
    }

    #[test]
    fn spawning_carves_the_largest_hole() {
        let mut w = world(Material::Dirt);
        w.spawn_worm(50, 50);
        assert_eq!(w.terrain.material(50, 50), Some(Material::Background));
        assert_eq!(w.terrain.material(43, 50), Some(Material::Background));
        assert_eq!(w.terrain.material(42, 50), Some(Material::Dirt));
        assert_eq!(w.worms()[0].dirt_count, 0, "spawning does not count as digging");
    }

    #[test]
    fn a_clone_evolves_identically() {
        let mut a = world(Material::Background);
        a.spawn_worm(100, 20);
        let mut b = a.clone();
        for k in [Keys(Keys::RIGHT), Keys(Keys::JUMP), Keys(0), Keys(Keys::ROPE | Keys::UP)] {
            a.tick(&[k]);
            b.tick(&[k]);
        }
        assert_eq!(a, b);
    }
}
```

- [ ] **Step 12: Replace `crates/rl-sim/src/lib.rs`**

```rust
//! The deterministic simulation (spec §5), ported from OpenLieroX's LX56
//! physics at commit `d1ad953f` and verified against the oracle traces in
//! `fixtures/oracle` (bit-exact).
//!
//! `#![no_std]` + `alloc`: no files, threads or clock. [`World::tick`] is the
//! only way state changes.
//!
//! Scope (M3): worm movement and collision, keyboard input, digging, the ninja
//! rope, and OLX's two clocks. Weapons, projectiles, bonuses and damage come in M4.
#![no_std]

extern crate alloc;

mod input;
mod physics;
mod rope;
mod settings;
mod world;
mod worm;

pub use input::{KeyLatch, Keys, Latches};
pub use rope::NinjaRope;
pub use settings::{SettingsError, SimSettings};
pub use world::World;
pub use worm::{Dir, Slot, Worm, WormState};
```

- [ ] **Step 13: Run**

```bash
RL_CONTENT_DIR=$PWD/target/content cargo nextest run -p rl-sim -p rl-scenario
```

Expected: output contains `10 passed`. `every_scenario_matches_olx_bit_for_bit` replays `aim`, `dig`, `fall`, `jump`, `rope`, `tap` and `walk` with no divergence. The world tests pin the two clocks, the Gusanos speed round trip (`0x40d33336` becomes `0x40d33335`), spawn carving, and that clones evolve identically.

- [ ] **Step 14: Run**

```bash
cargo xtask check --offline
```

Expected: output contains `check passed`. `wasm` also builds `rl-terrain` and `rl-sim`.

- [ ] **Step 15: Commit**

```bash
git add -A
git commit -m "rl-sim: port the LX56 worm, rope and clocks, oracle-exact"
```


---

### Task 6: `rl-cli scenario`, porting docs and the full gate

Adds the debugging entry point for divergences, and writes down what the port established: `docs/porting/timing.md`, `STATUS.md` and the root `CLAUDE.md`.

**Files:**

- Create: `crates/rl-cli/src/scenario.rs`, `crates/rl-cli/tests/scenario.rs`, `docs/porting/timing.md`
- Modify: `crates/rl-cli/{Cargo.toml,src/main.rs}`, `docs/porting/STATUS.md`, `CLAUDE.md`

**Interfaces:**

- Consumes: `rl_scenario::{fixture_names, load_fixture, replay}`, `rl_oracle::compare`.
- Produces: `rl-cli scenario [name...] [--content target/content] [--fixtures fixtures/oracle] [--dump <frame>]`. It prints `ok   <name>: <n> frames match OLX` or `DIFF <name>: <divergence>`; `--dump` prints `olx {...}` and `sim {...}` frame JSON; the exit status is 1 on any divergence.

- [ ] **Step 1: Create `crates/rl-cli/tests/scenario.rs`**

```rust
//! `rl-cli scenario` on the real fixtures (needs `RL_CONTENT_DIR`).

use std::path::PathBuf;
use std::process::Command;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/oracle")
}

fn scenario(content: &str, fixtures: &std::path::Path, extra: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rl-cli"))
        .args(["scenario", "--content", content, "--fixtures"])
        .arg(fixtures)
        .args(extra)
        .output()
        .unwrap()
}

#[test]
fn reports_matches_and_dumps_frames() {
    let Ok(content) = std::env::var("RL_CONTENT_DIR") else { return };
    let out = scenario(&content, &fixtures(), &["fall", "--dump", "8"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("ok   fall: 201 frames match OLX"), "{text}");
    assert!(text.contains("olx {\"f\":8,") && text.contains("sim {\"f\":8,"), "{text}");
}

#[test]
fn reports_a_divergence_with_status_1() {
    let Ok(content) = std::env::var("RL_CONTENT_DIR") else { return };
    let tmp = tempfile::tempdir().unwrap();
    std::fs::copy(fixtures().join("fall.ron"), tmp.path().join("fall.ron")).unwrap();
    let mut trace =
        rl_oracle::Trace::from_gz(&std::fs::read(fixtures().join("fall.jsonl.gz")).unwrap()).unwrap();
    trace.frames[50].w[0].vy = rl_oracle::Num(0.5);
    std::fs::write(tmp.path().join("fall.jsonl.gz"), rl_oracle::gzip(&trace.to_jsonl())).unwrap();
    let out = scenario(&content, tmp.path(), &[]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success());
    assert!(text.contains("DIFF fall: frame 49: w[0].vy expected 0.5"), "{text}");
}
```

- [ ] **Step 2: Create `crates/rl-cli/Cargo.toml`**

```toml
[package]
name = "rl-cli"
description = "Headless command-line tool: import OLX content, validate packs, replay oracle scenarios."
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish.workspace = true

[[bin]]
name = "rl-cli"
path = "src/main.rs"

[dependencies]
rl-data.workspace = true
rl-vfs.workspace = true
rl-pack.workspace = true
rl-import-lgs.workspace = true
rl-import-lxl.workspace = true
rl-import-gfx.workspace = true
rl-oracle.workspace = true
rl-scenario.workspace = true
serde_json.workspace = true
anyhow.workspace = true
clap.workspace = true
serde = { workspace = true, features = ["std"] }
ron.workspace = true

[dev-dependencies]
tempfile.workspace = true
insta.workspace = true

[lints]
workspace = true
```

- [ ] **Step 3: Run**

```bash
RL_CONTENT_DIR=$PWD/target/content cargo nextest run -p rl-cli --test scenario
```

 Expected to FAIL: there is no `scenario` subcommand yet, so both tests fail (`unrecognized subcommand`).

- [ ] **Step 4: Create `crates/rl-cli/src/scenario.rs`**

```rust
use std::path::PathBuf;

/// Replays oracle scenarios in the simulation and compares them with OLX.
#[derive(clap::Args)]
pub struct Args {
    /// Scenario names (`<fixtures>/<name>.ron`); all if none.
    names: Vec<String>,
    /// Imported content (packs written by `rl-cli import`).
    #[arg(long, default_value = "target/content")]
    content: PathBuf,
    /// Directory with the scenarios and OLX's traces.
    #[arg(long, default_value = "fixtures/oracle")]
    fixtures: PathBuf,
    /// Print OLX's and the simulation's state at this frame as JSON lines.
    #[arg(long)]
    dump: Option<i64>,
}

/// Returns `Ok(false)` if any scenario diverges from OLX.
pub fn run(args: &Args) -> anyhow::Result<bool> {
    let names =
        if args.names.is_empty() { rl_scenario::fixture_names(&args.fixtures)? } else { args.names.clone() };
    let mut ok = true;
    for name in names {
        let (sc, expected) = rl_scenario::load_fixture(&args.fixtures, &name)?;
        let actual = rl_scenario::replay(&args.content, &sc, &expected)?;
        match rl_oracle::compare(&expected, &actual, &sc.tolerances) {
            Ok(n) => println!("ok   {name}: {n} frames match OLX"),
            Err(d) => {
                ok = false;
                println!("DIFF {name}: {d}");
            }
        }
        if let Some(f) = args.dump {
            for (who, trace) in [("olx", &expected), ("sim", &actual)] {
                let frame = trace.frame(f).ok_or_else(|| anyhow::anyhow!("{name} has no frame {f}"))?;
                println!("{who} {}", serde_json::to_string(frame)?);
            }
        }
    }
    Ok(ok)
}
```

- [ ] **Step 5: Create `crates/rl-cli/src/main.rs`**

```rust
//! `rl-cli`: headless tools for rust-liero. Run `rl-cli --help`.

mod import;
mod scenario;
mod validate;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rl-cli", about = "Headless tools for rust-liero")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Import OpenLieroX content into native packs (one directory per pack).
    Import(import::Args),
    /// Replay oracle scenarios in the simulation and compare them with OLX's traces;
    /// exits with status 1 if any diverges.
    Scenario(scenario::Args),
    /// Validate packs; exits with status 1 if any pack has errors.
    Validate {
        /// Pack directories, or directories containing pack directories.
        paths: Vec<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Import(args) => import::run(&args),
        Command::Scenario(args) => scenario::run(&args),
        Command::Validate { paths } => validate::run(&paths),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}
```

- [ ] **Step 6: Run**

```bash
RL_CONTENT_DIR=$PWD/target/content cargo nextest run -p rl-cli --test scenario
```

Expected: output contains `2 passed`.

- [ ] **Step 7: Run**

```bash
cargo run -q -p rl-cli -- scenario
```

Expected: output contains `ok   walk: 331 frames match OLX`. All seven scenarios print `ok`.

- [ ] **Step 8: Create `docs/porting/timing.md`**

```markdown
# OLX timing, as ported in `rl-sim`

Spec §5.2 asked for this to be settled before any physics. Everything below
was confirmed bit for bit by the oracle (`fixtures/oracle`); sources are at
OLX commit `d1ad953f`.

## Time types

- `AbsTime` and `TimeDiff` are whole **milliseconds** in a `u64` (`include/olx-types.h:29-100`).
- **Seconds** are `ms * 0.001f`. So 20 ms is `20 * 0.001f`, which is **not** the
  same float as `0.02f`. `rl_math::TimeDiff::seconds` reproduces this.

## The two clocks

| Clock | OLX | Step | `rl-sim` |
|---|---|---|---|
| **Frame clock** | `Game::simulationTime`, read by `GetPhysicsTime()` | `Game::FixedFrameTime` = 10 ms | `World::time` |
| **Physics clock** | `CClient::fLastSimulationTime` | `LX56PhysicsDT` = `TimeDiff(1000 / LX56PhysicsFPS)` | `World::physics_time` |

- **The physics step is 11 ms, not 1/84 s.** `1000 / 84` is integer division,
  so the default step is 11 ms (about 91 Hz), and the worm's `dt` is
  `11 * 0.001f` = 0.0110000009.
- **Game speed** divides both steps (`TimeDiff * (1/speed)`, truncated to whole
  ms, at least 1) unless `GameSpeedOnlyForProjs` is set.

## One frame (`Game::frameInner`, `src/game/Game.cpp:897-959`)

1. **`serverFrame++`.**
2. **`gusLogicFrame()` inside a `GusSpeedScope`.** Every worm's and every rope's
   velocity is multiplied by `0.01f` (`GusDT.seconds()`), then by `1.0f / 0.01f`
   (= 99.9999924f). The round trip is lossy, for example 6.60000134 becomes
   6.60000086. It runs every frame, even for classic mods.
   `World::gus_speed_round_trip` reproduces it.
3. **The frame's keys apply.** In OLX this is the oracle's `beforeFrame`; in
   real play it is the input events.
4. **`CClient::Simulation()`** (`src/client/CClient_Game.cpp:60-222`): while
   `fLastSimulationTime + step <= simulationTime`, simulate every worm (its
   `getInput`, then physics), then advance `fLastSimulationTime` by one step.
5. **`cServer->Frame()`, then the oracle's `afterFrame`,** which records the state.
6. **`simulationTime += 10 ms`.**

With both clocks aligned when a scenario starts, frames 0 and 1 run no physics
step, frames 2 to 11 run one each, frame 12 runs none, and so on: the physics
clock falls 1 ms further behind every frame.

## Input timing

- **`getInput` runs inside each physics step**, so up to once per frame, and
  sometimes not at all in a frame.
- **Its `dt` uses the frame clock:** `GetPhysicsTime() - fLastInputTime`.
  After a spawn at the frame time T, the first read (in frame 2) uses
  `dt` = 20 ms.
- **Key latches:** see `docs/porting/STATUS.md`, "Facts the oracle established".

## Spawning

`GameServer::SpawnWorm` spawns the worm, and the host's own client spawns it
again when the spawn message arrives. Both happen at the same frame time and
position before the first recorded frame, and carving the same hole twice
changes nothing. So `World::spawn_worm` spawns once.
```

- [ ] **Step 9: Create `docs/porting/STATUS.md`**

```markdown
# Porting status

What has been ported from OpenLieroX (commit `d1ad953f`) and how it is verified.

| Area | OLX source | Rust | Status | Verified by |
|---|---|---|---|---|
| Gamescript v7 loader | `src/common/CGameScript.cpp:582-1152` | `rl-import-lgs` | ported | unit and property tests; Classic and MW 1.0 import exactly |
| `.lxl` image levels | `src/common/MapLoader_LieroX.cpp` | `rl-import-lxl` | ported | unit tests; 4 levels in scope, dirt counts match |
| Skin colorization | `src/client/CGameSkin.cpp:720-790` | `rl-data::skin`, `rl-import-gfx` | ported | unit tests; all 409 skins import |
| Theme loading | `src/common/CMap.cpp:308-370` | `rl-import-gfx` | ported | Dirt theme imports |
| Gamescript v8 | `CGameScript.cpp` (v8 branches) | — | not started | no content in scope |
| Pixmap `.lxl` | `MapLoader_LieroX.cpp:440+` | — | not started | no content in scope |
| Fidelity oracle (test hooks) | `src/game/Game.cpp`, `src/client/CInput.cpp`, `src/common/CWormHuman.cpp` | `tools/olx-oracle`, `rl-oracle` | done (plan 2) | 6 movement scenarios recorded twice and identical; `cargo xtask oracle-record --check` |
| Frame and physics clocks | `src/game/Game.cpp:897-959`, `src/client/CClient_Game.cpp:60-222` | `rl-sim::World::tick` | ported | all 7 oracle scenarios, bit for bit; `docs/porting/timing.md` |
| Worm movement and collision | `src/common/PhysicsLX56.cpp:88-469` | `rl-sim::physics` | ported | `fall`, `walk`, `jump`, `dig` |
| Keyboard input (aim, move, jump, dig, rope) | `src/common/CWormHuman.cpp:81-560`, `src/client/CInput.cpp` | `rl-sim::physics::get_input`, `rl-sim::input` | ported (keyboard, LX56 aim) | `aim`, `tap`, `walk`, `jump`, `dig`, `rope` |
| Ninja rope | `src/client/CNinjaRope.cpp`, `PhysicsLX56.cpp:505-604` | `rl-sim::rope` | ported | `rope` |
| Hole carving (material) | `src/common/CMap.cpp:1005-1105`, `2172-2199` | `rl-terrain::Terrain::carve_hole` | ported | `dig` (244 dirt cells), spawn holes (`mh` at frame −1) |
| Weapons, projectiles, bonuses, damage, `rand()` | `PhysicsLX56_Projectiles.cpp`, `CClient_Game.cpp` | — | not started (plan 4) | — |

## Deliberate deviations

- `rl-sim` supports LX56 aim only (`ForceLX56Aim`). It rejects wrap-around maps
  (`InfiniteMap`), Gusanos worm physics and `WormFriction > 0`
  (`SimSettings::from_olx_features` errors).
- Not yet emulated (M4): the `rand()` calls OLX makes for cosmetic particles
  when digging (`CarveHole(CVec)`) and when the rope first hooks dirt. They
  don't change M3 state, but they shift the random sequence that weapons will use.
- Unverified path: when the rope hook falls back towards the worm (it flew past
  `RopeMaxLength`), OLX normalizes with `1.0f/sqrt(length2)`. The port uses
  `sqrtf`; no scenario reaches this yet.

- Importers reject malformed input that OLX would read as garbage: truncated
  files, unknown weapon or projectile types, and nesting deeper than 128 levels.
- A missing projectile image is an import error. OLX would load the mod and draw nothing.
- Imported level art is stored at 1×, because OLX's 2× surfaces are plain pixel doubling.

## Kept OLX quirks

- **The physics step is 11 ms** (`1000 / 84`, integer division), not 1/84 s.
- **The lossy per-frame Gusanos speed round trip** (`GusSpeedScope`) on worm and
  rope velocities, from `gusLogicFrame()` (`docs/porting/timing.md`).
- **The collision scan continues instead of breaking** at the left and right
  map edges, and breaks at the top edge (`PhysicsLX56.cpp:132-206`). OLX's
  comments note that LX56 differed here.
- **`CarveHole` clipping:** near the map edge the stamp is read from its top-left
  corner, not offset by the clipped amount.

- `ROF /= 1000` and `Recharge /= 10` are applied after loading (`CGameScript.cpp:776-777`).
- Beam and pixel colors stored as ints are truncated to bytes, as with OLX's `Color(Uint8, …)`.
- Front pixels where the `PX_EMPTY` flag is set become the back pixel, even when other flags are set.
- **Colour key with a corner rule.** OLX uses (254,0,254) as the colour key when
  any corner pixel of the image has that colour, otherwise (255,0,255)
  (`SetColorKey`, `src/client/GfxPrimitives.cpp:116-150`).
  - Skins: applied at import.
  - Mod sprites and theme images: still stored raw, so the renderer must apply
    the same rule (plan 5). One affected file in scope is `skins/Wario.png`.
- **Hi-res level art.** With an "OLX hi-res data" chunk, the front 2×2 block
  becomes the back block wherever `PX_EMPTY` is set, and PNG alpha is ignored
  (`LoadLevelImageHiRes`, `MapLoader_LieroX.cpp:285-360`).

## Facts the oracle established (needed by the simulation port)

- **Frames:** a frame is 10 ms. LX56 physics runs in 84 Hz sub-steps from its
  own clock (`CClient::fLastSimulationTime`), so some 100 Hz frames run no
  physics sub-step at all. After the oracle aligns that clock when a scenario
  starts, frames 0 and 1 run none, and the first sub-step comes in frame 2.
  The step is 11 ms, not 1/84 s (see `docs/porting/timing.md`).
- **Input:** OLX's keyboard handler (`CWormHumanInputHandler::getInput`,
  `src/common/CWormHuman.cpp:81-560`) runs once per physics sub-step and
  latches keyboard events between sub-steps, then clears the latches with
  `CInput::reset()`.
  - **Held:** a key counts as held if it is down now *or* was pressed since the
    previous sub-step (`CInput::Binding::isDown`: `wasDown() || bDown`,
    `src/client/CInput.cpp:861-862`). A tap shorter than the gap between two
    sub-steps is still seen once (oracle scenario `tap`).
  - **Pressed once:** `isDownOnce` (`nDownOnce`) is true at the first sub-step after a press.
  - The simulation's `PlayerInput` must keep both latches.
- **Digging** with the keyboard is a tap of the opposite direction while
  holding a direction (`leftOnce` / `rightOnce`), not a dedicated key.
- **Aim:** the traces use LX56 aim (`ForceLX56Aim`: max speed 100,
  acceleration 500, no friction, `REDUCE_CONST(200)` when no key is held). The
  angle is clamped to [−90°, 60°].
```

- [ ] **Step 10: Create `CLAUDE.md`**

```markdown
# rust-liero

A Rust reimplementation of OpenLieroX (OLX) that feels identical and is
modernized for current displays.

- **Design:** `docs/superpowers/specs/2026-10-10-foundation-and-local-play-design.md`.
- **Plans:** `docs/superpowers/plans/`.
- **Porting status:** `docs/porting/STATUS.md`.

## Rules

1. **Run `cargo xtask check` before every commit.** It prints only failures.
   Without network access, use `cargo xtask check --offline`.
2. **One crate per concern, under `crates/`.** Each crate has a `CLAUDE.md`:
   read it before editing the crate. Register every new crate in `xtask/layers.ron`.
   A crate may depend only on lower layers (`cargo xtask layers`).
3. **Learn a dependency's API from its summary.** Run `cargo xtask api`, then
   read `target/api/<crate>.txt` instead of the crate's source.
4. **Cite OLX for ported behavior.** Ported code gets a doc comment naming the
   OLX source, such as `/// OLX: src/common/PhysicsLX56.cpp:272-469`, at
   commit `d1ad953f`. Faithfulness beats elegance; record deliberate deviations
   in `docs/porting/STATUS.md`.
5. **Keep the simulation pure.** Simulation-layer crates are `#![no_std]` and
   must also build for `wasm32-unknown-unknown`.
6. **Never panic on content.** Inputs from mods, levels and packs are untrusted.
7. **Keep output deterministic.** Use no `HashMap` iteration in output, and no wall clock in logic.
8. **Commits:** small, imperative subject (`rl-pack: validate projectile cycles`).

## Oracle

`fixtures/oracle/<name>.ron` are scenarios, and `<name>.jsonl.gz` are their
traces recorded from a patched OLX (`tools/olx-oracle/`).
- `cargo xtask oracle-record [name...]` re-records them; it needs Docker.
- `--check` compares fresh recordings with the committed traces.
- The simulation must reproduce the traces bit-exactly (crate `rl-oracle`).
- `rl-cli scenario [name...] [--dump <frame>]` replays scenarios in `rl-sim`
  and reports the first divergence. Use it when a change breaks the oracle.

## Content

`cargo xtask fetch-olx-content` sparse-clones OLX at the commit pinned in
`content/olx-manifest.ron` into `target/olx/<commit>/`, then imports the
listed items into `target/content/`. Converted assets are never committed.
```

- [ ] **Step 11: Run**

```bash
cargo xtask check
```

Expected: output contains `check passed`. The online gate runs the acceptance test with real content.

- [ ] **Step 12: Commit**

```bash
git add -A
git commit -m "Add rl-cli scenario and porting notes for the simulation"
```
