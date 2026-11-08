#!/bin/bash
# Live part of the demo video: boot the QNX 8.0 x86_64 VM (QEMU/KVM in WSL2), stream its serial console
# while it boots, then run the example programs on it over ssh. Everything printed here happens live.
#   wsl -d Ubuntu-24.04 -u root -- bash scripts/demo/vm_live.sh <wsl path to QNX x86_64 binary>...
source /root/qnx800/qnxsdp-env.sh >/dev/null
brctl show | grep -q ^br0 || bash $QNX_BASE/host/common/mkqnximage/qemu/net.sh /usr/lib/qemu/qemu-bridge-helper /etc/qemu/bridge.conf nat >/dev/null 2>&1
pgrep -f 'dnsmasq.*br0' >/dev/null || dnsmasq --interface=br0 --bind-interfaces --except-interface=lo --listen-address=172.31.1.1 --port=0 --dhcp-range=172.31.1.2,172.31.1.254 --pid-file=/run/dnsmasq-br0.pid
P() { printf '\033[32mroot@wsl\033[0m:\033[34m%s\033[0m# %s\n' "$PWD" "$*"; }
Q() { printf '\033[33mroot@qnx\033[0m:/tmp# %s\n' "$*"; }
cd /root/qnx-vm-x86_64
P "mkqnximage --run=-h    # QNX 8.0 x86_64 under qemu-system-x86_64 -enable-kvm"
rm -f output/qemu.out
mkqnximage --noprompt --run=-h > run.log 2>&1 < /dev/null
for i in $(seq 1 50); do [ -f output/qemu.out ] && break; sleep 0.2; done
P "tail -f output/qemu.out    # serial console"
# the console starts with ESC c (full reset) and a clear-screen; drop both so the lines above stay visible
tail -n +1 -F output/qemu.out 2>/dev/null > >(sed -u -e 's/\x1bc//g' -e 's/\x1b\[2J//g') & T=$!
for i in $(seq 1 120); do grep -q "Startup complete" output/qemu.out 2>/dev/null && break; sleep 0.5; done
sleep 1; kill $T 2>/dev/null; wait $T 2>/dev/null; echo
IP=$(mkqnximage --getip 2>/dev/null | grep -Eo '172\.31\.1\.[0-9]+' | head -1)
[ -z "$IP" ] && IP=$(grep -Eo '172\.31\.1\.[0-9]+' /var/lib/misc/dnsmasq.leases | head -1)
O="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=8 -i /root/.ssh/id_ed25519"
for i in $(seq 1 20); do ssh $O root@$IP true < /dev/null 2>/dev/null && break; sleep 1; done
P "scp $(for b in "$@"; do printf '%s ' "$(basename $b)"; done)root@$IP:/tmp/"
scp -q $O "$@" root@$IP:/tmp/ < /dev/null
Q "uname -a"; ssh $O root@$IP 'uname -a' < /dev/null
for b in "$@"; do n=$(basename $b); Q "./$n"; ssh $O root@$IP "cd /tmp && ./$n; echo exit code \$?" < /dev/null; done
P "mkqnximage --stop"
mkqnximage --stop >/dev/null 2>&1; pkill -f qemu-system-x86_64; echo "VM stopped"
