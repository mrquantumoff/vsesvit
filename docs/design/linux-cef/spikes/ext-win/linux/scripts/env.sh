# Sourced by every Linux script. Paths come from VSESVIT_SCRATCH (default ~/vsesvit-scratch).
SCRATCH=${VSESVIT_SCRATCH:-$HOME/vsesvit-scratch}
XW=$SCRATCH/ext-win
BIN=$XW/target/debug/vsesvit-extwin
XCTL=$XW/target/debug/xctl
FX=$XW/fx
OPT=$SCRATCH/opt/usr/bin
DPY=:94
LVP=$(ls /usr/share/vulkan/icd.d/lvp_icd*.json 2>/dev/null | head -1)
[ -f $FX/ids.env ] && source $FX/ids.env
port_ctl() { case $1 in A) echo 47911;; B) echo 47912;; esac; }
port_rep() { case $1 in A) echo 47921;; B) echo 47922;; esac; }
# c PROFILE LINE: one control line, prints the reply.
c() { python3 -I -c 'import socket,sys; s=socket.create_connection(("127.0.0.1",int(sys.argv[1]))); s.settimeout(130); s.sendall((sys.argv[2]+"\n").encode()); print(s.makefile().readline().rstrip())' "$(port_ctl $1)" "$2"; }
