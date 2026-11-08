#!/bin/bash
# One WSL session: start QNX x86_64 QEMU VM headless, wait for boot, run on-target tests, stop VM.
source /root/qnx800/qnxsdp-env.sh >/dev/null
brctl show | grep -q ^br0 || bash /root/qnx800/host/common/mkqnximage/qemu/net.sh /usr/lib/qemu/qemu-bridge-helper /etc/qemu/bridge.conf nat >/dev/null 2>&1
pgrep -f 'dnsmasq.*br0' >/dev/null || dnsmasq --interface=br0 --bind-interfaces --except-interface=lo --listen-address=172.31.1.1 --port=0 --dhcp-range=172.31.1.2,172.31.1.254 --pid-file=/run/dnsmasq-br0.pid
cd /root/qnx-vm-x86_64
rm -f /var/lib/misc/dnsmasq.leases.tmp
mkqnximage --noprompt --run=-h > run.log 2>&1 < /dev/null
for i in $(seq 1 40); do grep -q "Startup complete" output/qemu.out 2>/dev/null && break; sleep 2; done
grep -a -E "sshd|Startup complete|QNX " output/qemu.out | tail -4
sleep 3
IP=$(mkqnximage --getip 2>/dev/null | grep -Eo '172\.31\.1\.[0-9]+' | head -1)
[ -z "$IP" ] && IP=$(grep -Eo '172\.31\.1\.[0-9]+' /var/lib/misc/dnsmasq.leases | head -1)
bash /mnt/c/Users/Matthew/outreach/competitions/hack_bounty/qnx/scripts/vm_test.sh $IP "$@"
mkqnximage --stop >/dev/null 2>&1; pkill -f qemu-system-x86_64
echo VM_STOPPED
