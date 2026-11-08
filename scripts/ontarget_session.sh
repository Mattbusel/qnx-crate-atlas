#!/bin/bash
# One WSL session: boot the QNX 8.0 x86_64 QEMU VM, run a list of test binaries on it one at a time,
# stop the VM. WSL stops the VM when the wsl.exe call returns, so everything happens in this one call.
#   ontarget_session.sh <manifest>     manifest lines: "<label> <wsl path to QNX x86_64 binary>"
# Output per binary: "@@BEGIN <label>", the binary's own output, "@@END <label> RC=<code>".
# Needs: /root/qnx800 (SDP 8.0 Linux host), /root/qnx-vm-x86_64 (made by mkqnximage), key /root/.ssh/id_ed25519.
MANIFEST=$1
PER_TEST_TIMEOUT=${PER_TEST_TIMEOUT:-240}
VM_DIR=${VM_DIR:-/root/qnx-vm-x86_64}
source /root/qnx800/qnxsdp-env.sh >/dev/null
brctl show | grep -q ^br0 || bash $QNX_BASE/host/common/mkqnximage/qemu/net.sh /usr/lib/qemu/qemu-bridge-helper /etc/qemu/bridge.conf nat >/dev/null 2>&1
pgrep -f 'dnsmasq.*br0' >/dev/null || dnsmasq --interface=br0 --bind-interfaces --except-interface=lo --listen-address=172.31.1.1 --port=0 --dhcp-range=172.31.1.2,172.31.1.254 --pid-file=/run/dnsmasq-br0.pid
cd $VM_DIR
mkqnximage --noprompt --run=-h > run.log 2>&1 < /dev/null
for i in $(seq 1 60); do grep -q "Startup complete" output/qemu.out 2>/dev/null && break; sleep 2; done
sleep 3
IP=$(mkqnximage --getip 2>/dev/null | grep -Eo '172\.31\.1\.[0-9]+' | head -1)
[ -z "$IP" ] && IP=$(grep -Eo '172\.31\.1\.[0-9]+' /var/lib/misc/dnsmasq.leases | head -1)
O="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=8 -i /root/.ssh/id_ed25519"
for i in $(seq 1 20); do ssh $O root@$IP true < /dev/null 2>/dev/null && break; sleep 2; done
echo "@@VM $(ssh $O root@$IP 'uname -a' < /dev/null)"
while read -r label path; do
  [ -z "$label" ] && continue
  n=$(basename "$path")
  echo "@@BEGIN $label"
  if scp -q $O "$path" root@$IP:/tmp/$n < /dev/null; then
    timeout $PER_TEST_TIMEOUT ssh $O root@$IP "cd /tmp && ./$n --test-threads=2 2>&1; echo @@RC=\$?; rm -f /tmp/$n" < /dev/null
    rc=$?
    [ $rc -eq 124 ] && echo "@@RC=timeout"
  else
    echo "@@RC=scp-failed"
  fi
  echo "@@END $label"
done < "$MANIFEST"
mkqnximage --stop >/dev/null 2>&1; pkill -f qemu-system-x86_64
echo "@@VM_STOPPED"
