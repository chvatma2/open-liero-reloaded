// The relay's rate limits, counted in KV: a few reports per client
// address per hour, and a cap on issues per UTC day in all. KV is
// eventually consistent, so a burst across edge locations may pass a
// little over; Cloudflare's rate-limiting binding only counts 10- or
// 60-second windows. The client's address is stored hashed.

export const DEFAULT_PER_IP_PER_HOUR = 3;
export const DEFAULT_PER_DAY = 50;

function positive(v, fallback) {
  const n = Number.parseInt(v ?? '', 10);
  return Number.isSafeInteger(n) && n > 0 ? n : fallback;
}

async function sha256Hex(text) {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

/**
 * Counts a report from `ip` at `nowMs` if both limits allow it: `true`
 * when counted, `false` when over either (nothing counted then).
 */
export async function admit(kv, env, ip, nowMs) {
  const perIp = positive(env.PER_IP_PER_HOUR, DEFAULT_PER_IP_PER_HOUR);
  const perDay = positive(env.PER_DAY, DEFAULT_PER_DAY);
  const hour = Math.floor(nowMs / 3_600_000);
  const day = new Date(nowMs).toISOString().slice(0, 10);
  const ipKey = `ip:${await sha256Hex(`${env.IP_SALT ?? ''}|${ip}`)}:${hour}`;
  const dayKey = `day:${day}`;
  const [ipCount, dayCount] = (await Promise.all([kv.get(ipKey), kv.get(dayKey)])).map((v) => Number(v ?? 0) || 0);
  if (ipCount >= perIp || dayCount >= perDay) return false;
  await Promise.all([
    kv.put(ipKey, String(ipCount + 1), { expirationTtl: 2 * 3600 }),
    kv.put(dayKey, String(dayCount + 1), { expirationTtl: 2 * 86_400 }),
  ]);
  return true;
}
