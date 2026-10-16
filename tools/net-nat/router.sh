#!/bin/sh
# router.sh <kind> <wan ip> <lan ip>: a home router between a LAN and the
# test WAN (`cargo xtask net-nat`, tools/net-nat/CLAUDE.md).
#   cone       nftables masquerade: a port-restricted cone NAT (the port
#              kept per local address and port, replies only from where
#              it sent)
#   symmetric  masquerade fully-random: a new port per destination
#   upnp       cone, plus miniupnpd with UPnP-IGD only
#   pcp        cone, plus miniupnpd with NAT-PMP and PCP only
#   pmp        as pcp, but PCP requests (version byte 2) are dropped: a
#              NAT-PMP-only router (miniupnpd switches both together)
#   hairpin    cone, plus a manual forward of a TURN relay's ports (UDP
#              3478 and 49160-49191) to the LAN's .30, with hairpin NAT:
#              the LAN reaches them at the WAN address too, and its
#              packets so forwarded come from the WAN address (TR-5: a
#              relay beside the host)
# (olr-net asks every method at once and keeps the first mapping: PCP
# answers before UPnP's search ends, so a router offering both would
# never show UPnP.)
# Prints "router ready" once it routes (and miniupnpd listens), then
# runs until stopped or for OLR_NAT_LIFE seconds (default 120): a run
# that was interrupted leaves no router running for long.
set -eu
kind=$1
wan_ip=$2
lan_ip=$3

# The interface holding an address (Docker names them in the order the
# networks were attached, so they are found by address).
iface_of() {
    ip -o -4 addr show | awk -v want="$1" '{ split($4, a, "/"); if (a[1] == want) { sub(/@.*/, "", $2); print $2 } }'
}
wan=$(iface_of "$wan_ip")
lan=$(iface_of "$lan_ip")
if [ -z "$wan" ] || [ -z "$lan" ]; then
    echo "router: no interface holds $wan_ip or $lan_ip" >&2
    ip -o -4 addr show >&2
    exit 1
fi
lan_net=$(ip -o -4 route show dev "$lan" scope link | awk '{ print $1; exit }')

case "$kind" in
    symmetric) masquerade="masquerade fully-random" ;;
    cone | upnp | pcp | pmp | hairpin) masquerade="masquerade" ;;
    *)
        echo "router: unknown kind $kind" >&2
        exit 1
        ;;
esac

# A home router's rules: the LAN goes out masqueraded, only replies (and
# miniupnpd's forwards, in its chains) come in. The chain names are
# miniupnpd's defaults (table inet filter). Unsolicited WAN packets to the
# router itself are dropped, as every home router does: dropped in input,
# their conntrack entry is never confirmed. Accepted (to a closed port),
# it would be, and an outbound flow to that peer would then be masqueraded
# to another port: hole punching would fail where a real router lets it.
# A NAT-PMP-only router drops PCP: the first payload byte (after the
# 8-byte UDP header) is the version, 2 for PCP and 0 for NAT-PMP.
pcp_rule=""
if [ "$kind" = pmp ]; then
    pcp_rule="iifname \"$lan\" udp dport 5351 @th,64,8 2 drop"
fi
# A manual forward of the relay's ports, from the WAN and (hairpin) from
# the LAN: forwarded packets are let through, and a LAN packet forwarded
# back into the LAN leaves from the WAN address, as a hairpinning home
# router does.
forward_rule="" dnat_rule="" hairpin_rule=""
if [ "$kind" = hairpin ]; then
    relay_ip=$(echo "$lan_ip" | sed 's/\.[0-9]*$/.30/')
    forward_rule="ct status dnat accept"
    dnat_rule="ip daddr $wan_ip udp dport { 3478, 49160-49191 } dnat ip to $relay_ip"
    hairpin_rule="ip saddr $lan_net ip daddr $lan_net ct status dnat snat ip to $wan_ip"
fi
nft -f - <<NFT
table inet filter {
    chain input {
        type filter hook input priority 0; policy drop;
        ct state established,related accept
        iif "lo" accept
        $pcp_rule
        iifname "$lan" accept
    }
    chain forward {
        type filter hook forward priority 0; policy drop;
        ct state established,related accept
        iifname "$lan" oifname "$wan" accept
        $forward_rule
        jump miniupnpd
    }
    chain miniupnpd {
    }
    chain prerouting {
        type nat hook prerouting priority -100; policy accept;
        $dnat_rule
        jump prerouting_miniupnpd
    }
    chain postrouting {
        type nat hook postrouting priority 100; policy accept;
        jump postrouting_miniupnpd
        $hairpin_rule
        oifname "$wan" $masquerade
    }
    chain prerouting_miniupnpd {
    }
    chain postrouting_miniupnpd {
    }
}
NFT

if [ "$kind" = upnp ] || [ "$kind" = pcp ] || [ "$kind" = pmp ]; then
    if [ "$kind" = upnp ]; then
        upnp=yes pcp=no port=1900
    else
        upnp=no pcp=yes port=5351
    fi
    sed -e "s|@WAN@|$wan|g" -e "s|@WAN_IP@|$wan_ip|g" -e "s|@LAN@|$lan|g" -e "s|@LAN_NET@|$lan_net|g" -e "s|@UPNP@|$upnp|g" \
        -e "s|@PCP@|$pcp|g" /olr-nat/miniupnpd.conf.in >/tmp/miniupnpd.conf
    miniupnpd -d -f /tmp/miniupnpd.conf -P /tmp/miniupnpd.pid &
    # Ready once it listens: SSDP (UDP 1900) for UPnP, else NAT-PMP and
    # PCP (UDP 5351).
    n=0
    until ss -lun | grep -q ":$port "; do
        n=$((n + 1))
        if [ $n -gt 100 ]; then
            echo "router: miniupnpd did not start" >&2
            exit 1
        fi
        sleep 0.05
    done
fi
echo "router ready: $kind, WAN $wan $wan_ip, LAN $lan $lan_ip ($lan_net)"
# Until stopped (docker stop: --init forwards SIGTERM) or its lifetime ends.
sleep "${OLR_NAT_LIFE:-120}"
