#!/system/bin/sh

SCRIPT_DIR=${0%/*}
MODDIR=${SCRIPT_DIR%/*}
CONFIG_FILE="$MODDIR/config/config.toml"
MODE_FILE="$MODDIR/config/config.txt"
LOG_FILE="$MODDIR/log/log.log"
BINARY="$MODDIR/system/bin/finalizer"

fail() {
    echo "$1" >&2
    exit 1
}

read_log_level_from() {
    awk '
        /^\[log\][[:space:]]*$/ { in_log = 1; next }
        /^\[/ { in_log = 0 }
        in_log && /^[[:space:]]*level[[:space:]]*=/ {
            value = $0
            sub(/^[^=]*=/, "", value)
            sub(/[[:space:]]*#.*/, "", value)
            gsub(/[[:space:]"]/, "", value)
            print value
            exit
        }
    ' "$1"
}

validate_level() {
    case "$1" in
        error|warn|info|debug) return 0 ;;
        *) return 1 ;;
    esac
}

validate_config_file() {
    file=$1
    [ -s "$file" ] || return 1
    grep -q '^\[name\][[:space:]]*$' "$file" || return 1
    grep -q '^\[\[policy\]\][[:space:]]*$' "$file" || return 1
    grep -q '^\[mode\.power\][[:space:]]*$' "$file" || return 1
    grep -q '^\[mode\.blan\][[:space:]]*$' "$file" || return 1
    grep -q '^\[mode\.perf\][[:space:]]*$' "$file" || return 1
    grep -q '^\[mode\.fast\][[:space:]]*$' "$file" || return 1
    level=$(read_log_level_from "$file")
    [ -z "$level" ] || validate_level "$level" || return 1
    [ -x "$BINARY" ] || return 1
    "$BINARY" --validate-config "$file" >/dev/null 2>&1
}

backup_config() {
    cp "$CONFIG_FILE" "$CONFIG_FILE.webui.bak" || fail "备份配置失败"
}

set_log_level() {
    level=$1
    validate_level "$level" || fail "不支持的日志级别"
    tmp="$CONFIG_FILE.webui.$$"

    awk -v level="$level" '
        BEGIN { in_log = 0; found_log = 0; changed = 0 }
        /^\[log\][[:space:]]*$/ {
            in_log = 1
            found_log = 1
            print
            next
        }
        /^\[/ {
            if (in_log && !changed) {
                print "level = \"" level "\""
                changed = 1
            }
            in_log = 0
        }
        in_log && /^[[:space:]]*level[[:space:]]*=/ {
            print "level = \"" level "\""
            changed = 1
            next
        }
        { print }
        END {
            if (in_log && !changed) {
                print "level = \"" level "\""
            } else if (!found_log) {
                print ""
                print "[log]"
                print "level = \"" level "\""
            }
        }
    ' "$CONFIG_FILE" > "$tmp" || {
        rm -f "$tmp"
        fail "更新日志级别失败"
    }

    validate_config_file "$tmp" || {
        rm -f "$tmp"
        fail "更新后的配置无效"
    }
    backup_config
    cat "$tmp" > "$CONFIG_FILE" || {
        rm -f "$tmp"
        fail "写入配置失败"
    }
    rm -f "$tmp"
    echo "$level"
}

write_config() {
    payload=$1
    [ -n "$payload" ] || fail "配置内容为空"
    tmp="$CONFIG_FILE.webui.$$"

    printf '%s' "$payload" | base64 -d > "$tmp" 2>/dev/null || {
        rm -f "$tmp"
        fail "配置解码失败"
    }
    validate_config_file "$tmp" || {
        rm -f "$tmp"
        fail "配置缺少必要字段或日志级别无效"
    }
    backup_config
    cat "$tmp" > "$CONFIG_FILE" || {
        rm -f "$tmp"
        fail "写入配置失败"
    }
    rm -f "$tmp"
    echo "saved"
}

restart_finalizer() {
    pids=$(pidof finalizer 2>/dev/null)
    if [ -n "$pids" ]; then
        kill $pids 2>/dev/null || fail "停止 Finalizer 失败"
        count=0
        while pidof finalizer >/dev/null 2>&1 && [ "$count" -lt 20 ]; do
            sleep 0.1
            count=$((count + 1))
        done
    fi
    pidof finalizer >/dev/null 2>&1 && fail "Finalizer 未能停止"

    "$BINARY" </dev/null >/dev/null 2>&1 &
    sleep 2
    if ! pidof finalizer >/dev/null 2>&1; then
        backup="$CONFIG_FILE.webui.bak"
        if [ -f "$backup" ] && validate_config_file "$backup"; then
            cat "$backup" > "$CONFIG_FILE" || fail "Finalizer 启动失败，配置回滚也失败"
            "$BINARY" </dev/null >/dev/null 2>&1 &
            sleep 2
            if pidof finalizer >/dev/null 2>&1; then
                fail "新配置启动失败，已恢复上一份配置"
            fi
        fi
        fail "Finalizer 启动失败，请检查日志和配置"
    fi
    echo "running"
}

case "${1:-}" in
    status)
        mode=$(head -n 1 "$MODE_FILE" 2>/dev/null | tr -d '\r\n')
        level=$(read_log_level_from "$CONFIG_FILE")
        [ -n "$level" ] || level=info
        if pidof finalizer >/dev/null 2>&1; then
            running=1
        else
            running=0
        fi
        if [ -f "$LOG_FILE" ]; then
            log_bytes=$(wc -c < "$LOG_FILE" | tr -d ' ')
        else
            log_bytes=0
        fi
        echo "mode=${mode:-unknown}"
        echo "log_level=$level"
        echo "running=$running"
        echo "log_bytes=$log_bytes"
        ;;
    set-mode)
        case "${2:-}" in
            powersave|balance|performance|fast)
                printf '%s\n' "$2" > "$MODE_FILE" || fail "写入模式失败"
                echo "$2"
                ;;
            *) fail "不支持的运行模式" ;;
        esac
        ;;
    set-log-level)
        set_log_level "${2:-}"
        ;;
    tail-log)
        [ -f "$LOG_FILE" ] && tail -n 250 "$LOG_FILE"
        ;;
    clear-log)
        mkdir -p "${LOG_FILE%/*}" || fail "创建日志目录失败"
        : > "$LOG_FILE" || fail "清空日志失败"
        echo "cleared"
        ;;
    read-config)
        cat "$CONFIG_FILE" || fail "读取配置失败"
        ;;
    write-config)
        write_config "${2:-}"
        ;;
    restart)
        restart_finalizer
        ;;
    *)
        fail "未知操作"
        ;;
esac
