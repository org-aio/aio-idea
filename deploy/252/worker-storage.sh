#!/bin/sh
set -eu

# 归档必须使用数据盘，挂载失败时禁止悄悄写入系统盘。
source_dir=/home/data/aio-space/managed
target_dir=/opt/aio-idea/worker-storage
test "$(id -u)" -eq 0
mountpoint -q /home
install -d -o aio-shell -g aio-shell -m 0700 "$source_dir"
if ! mountpoint -q "$target_dir"; then
    install -d -o aio-shell -g aio-shell -m 0700 "$target_dir"
    test -z "$(ls -A "$target_dir")"
    mount --bind "$source_dir" "$target_dir"
fi
test "$(stat -c '%d:%i' "$source_dir")" = "$(stat -c '%d:%i' "$target_dir")"
entry="$source_dir $target_dir none bind 0 0"
if ! grep -Fxq "$entry" /etc/fstab; then
    if awk -v target="$target_dir" '$1 !~ /^#/ && $2 == target {found=1} END {exit !found}' /etc/fstab; then
        echo 'Existing fstab entry differs; inspect worker storage mount' >&2
        exit 1
    fi
    cp -p /etc/fstab "/etc/fstab.aio-worker.$(date +%s)"
    printf '\n%s\n' "$entry" >> /etc/fstab
fi
systemctl daemon-reload
df -h "$target_dir"
