#!/bin/bash
# usage (inside WSL, VM running): vm_test.sh <vm-ip> <qnx-binary> [<qnx-binary>...]
# Copies each binary to /tmp on the QNX VM over ssh and runs it.
IP=$1; shift
[ -z "$IP" ] && IP=$(grep -Eo '172\.31\.1\.[0-9]+' /var/lib/misc/dnsmasq.leases | head -1)
echo "VM_IP=$IP"
O="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=5 -i /root/.ssh/id_ed25519"
ssh $O root@$IP 'uname -a' < /dev/null
[ $# -eq 0 ] && set -- /mnt/c/Users/Matthew/.cargo/cargo-target/x86_64-pc-nto-qnx800/release/hello-qnx /mnt/c/Users/Matthew/.cargo/cargo-target/x86_64-pc-nto-qnx800/release/deptest
scp $O "$@" root@$IP:/tmp/ < /dev/null; echo SCP_RC=$?
for b in "$@"; do n=$(basename $b); ssh $O root@$IP "cd /tmp && ./$n; echo ${n}_RC=\$?" < /dev/null; done
