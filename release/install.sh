#!/bin/sh
# Canonical user installer: /bin/sh install.sh [--version V] [--install-dir DIR]
#   [--bin-dir DIR] [--no-modify-path] [--uninstall]
# Test fixtures alone may set ILIUM_INSTALL_TEST_ORIGIN to an HTTPS IPv4
# loopback origin ending /releases. Production always uses immutable tagged
# assets at https://github.com/arthurwolf/ilium/releases. No archive code runs.
# SC2016: generated launcher/profile source must retain literal dollar signs.
# shellcheck disable=SC2016
set -eu
umask 077
stage=arguments
requested_version=latest
version=
target=unknown
prior_active=no
work=
lock_owned=no
committed=no
profile_added=no
profile_metadata=no
profile_publishing=no
new_receipt=no
previous=
new_client=no
new_server=no
launcher_temp=
repair_temp=
profile_temp=
receipt_temp=
previous_temp=
diagnosed=no
user_home=${HOME:-}
install_root=${XDG_DATA_HOME:-$user_home/.local/share}/ilium
bin_dir=${XDG_BIN_HOME:-$user_home/.local/bin}
modify_path=yes
uninstall=no

recovery_quote() {
    # Diagnostics must also work when a required external tool is missing.
    recovery_remaining=$1
    printf "'"
    while :; do
        case "$recovery_remaining" in
            *"'"*)
                recovery_prefix=${recovery_remaining%%"'"*}
                printf '%s%s' "$recovery_prefix" "'\\''"
                recovery_remaining=${recovery_remaining#*"'"} ;;
            *) printf "%s'" "$recovery_remaining"; break ;;
        esac
    done
}

diagnostic() {
    printf 'ilium-install: stage=%s version=%s target=%s prior_active=%s error=%s\n' "$stage" "$requested_version" "$target" "$prior_active" "$1" >&2
    printf "ilium-install: recovery=curl --proto '=https' --tlsv1.2 -LsSf https://ilium-setup.pages.dev/install.sh | sh -s --" >&2
    if [ -n "$version" ]; then printf ' --version ' >&2; recovery_quote "$version" >&2; fi
    printf ' --install-dir ' >&2; recovery_quote "$install_root" >&2
    printf ' --bin-dir ' >&2; recovery_quote "$bin_dir" >&2
    if [ "$uninstall" = yes ]; then printf ' --uninstall' >&2; fi
    printf ' --no-modify-path\n' >&2
}
fail() {
    diagnosed=yes
    diagnostic "$1"
    exit 1
}

valid_version() {
    printf '%s\n' "$1" | LC_ALL=C awk 'NR != 1 {bad=1} !/^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?$/ {bad=1} END {exit (bad || NR != 1)}'
}

valid_path() {
    case "$1" in /*) ;; *) return 1 ;; esac
    case "$1" in /|*":"*|*/../*|*/..|*/./*|*/.) return 1 ;; esac
    # Reject control characters; safe shell quoting preserves apostrophes,
    # spaces and Unicode without evaluating authored path bytes.
    [ "$(printf '%s' "$1" | tr -d '\000-\037\177')" = "$1" ]
}

quote() { printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"; }

valid_path "$user_home" || fail "HOME must be a safe absolute user directory"

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version|--install-dir|--bin-dir)
            [ "$#" -ge 2 ] || fail "Missing flag value"
            case "$1" in
                --version) requested_version=$2; version=${2#v}; valid_version "$version" || fail "Invalid semantic version" ;;
                --install-dir) install_root=$2 ;;
                --bin-dir) bin_dir=$2 ;;
            esac
            shift 2 ;;
        --no-modify-path) modify_path=no; shift ;;
        --uninstall) uninstall=yes; shift ;;
        --help)
            printf 'Usage: /bin/sh install.sh [--version V] [--install-dir DIR] [--bin-dir DIR] [--no-modify-path] [--uninstall]\n'
            exit 0 ;;
        *) fail "Unknown argument" ;;
    esac
done
install_root=${install_root%/}
bin_dir=${bin_dir%/}
if ! valid_path "$install_root" || ! valid_path "$bin_dir"; then fail "Directories must be safe absolute paths"; fi
case "$bin_dir/" in "$install_root/"*) fail "Stable bin directory must be outside installation root" ;; esac
case "$install_root/" in "$bin_dir/"*) fail "Installation root must be outside stable bin directory" ;; esac

stage=target
kernel=$(uname -s) || fail "Cannot determine kernel"
architecture=$(uname -m) || fail "Cannot determine architecture"
case "$architecture" in arm64) architecture=aarch64 ;; esac
# BEGIN GENERATED POSIX TARGETS
checksum_archives='ilium-linux-x86_64.tar.gz ilium-linux-aarch64.tar.gz ilium-windows-x86_64.zip ilium-macos-aarch64.tar.gz ilium-macos-x86_64.tar.gz'
case "$kernel/$architecture" in
    Linux/x86_64) archive_name=ilium-linux-x86_64.tar.gz; target_os=linux; target=x86_64-unknown-linux-gnu ;;
    Linux/aarch64) archive_name=ilium-linux-aarch64.tar.gz; target_os=linux; target=aarch64-unknown-linux-gnu ;;
    Darwin/aarch64) archive_name=ilium-macos-aarch64.tar.gz; target_os=macos; target=aarch64-apple-darwin ;;
    Darwin/x86_64) archive_name=ilium-macos-x86_64.tar.gz; target_os=macos; target=x86_64-apple-darwin ;;
    *) fail "Unsupported target; supported: Linux/x86_64, Linux/aarch64, Darwin/aarch64, Darwin/x86_64" ;;
esac
# END GENERATED POSIX TARGETS
if [ "$target_os" = linux ]; then
    libc_identity=$(getconf GNU_LIBC_VERSION 2>/dev/null) || fail "Linux release requires glibc; musl or unverified libc is unsupported"
    case "$libc_identity" in 'glibc '[0-9]*.[0-9]*) ;; *) fail "Linux release requires glibc; musl or unverified libc is unsupported" ;; esac
fi

stage=prerequisites
for command_name in awk cat chmod cmp cp dd df dirname find grep gzip mkdir mktemp mv od rm rmdir sed sort tar tr wc; do
    command -v "$command_name" >/dev/null 2>&1 || fail "Missing required utility: $command_name"
done
if command -v sha256sum >/dev/null 2>&1; then hash_tool=sha256sum
elif command -v shasum >/dev/null 2>&1; then hash_tool=shasum
elif command -v openssl >/dev/null 2>&1; then hash_tool=openssl
else hash_tool=
fi
require_hash() { [ -n "$hash_tool" ] || fail "A SHA-256 tool is required: sha256sum, shasum, or openssl"; }
# A fresh missing-tool failure must not leave an unowned skeleton that blocks
# retry. Existing owned installs defer this error until their locked prior read.
if [ ! -f "$install_root/installer-state/owner" ]; then require_hash; fi
hash_file() {
    case "$hash_tool" in
        sha256sum) sha256sum < "$1" ;;
        shasum) shasum -a 256 < "$1" ;;
        openssl) openssl dgst -sha256 < "$1" ;;
    esac | awk 'NR==1 {hash=$NF; if(length($1)==64)hash=$1} END {if(NR!=1 || length(hash)!=64 || hash!~/^[0-9a-f]+$/)exit 1;print hash}'
}

stage=ownership
if [ -e "$install_root/.install-lock" ] || [ -L "$install_root/.install-lock" ]; then stage=lock; fail "Another installer owns the lock; wait for it to finish"; fi
for directory in "$install_root" "$install_root/versions" "$install_root/installer-state" "$bin_dir"; do
    [ ! -L "$directory" ] || fail "Installation directories cannot be symlinks"
    [ ! -e "$directory" ] || [ -d "$directory" ] || fail "Installation path is not a directory"
done
state=$install_root/installer-state
if [ -d "$install_root" ] && [ ! -f "$state/owner" ]; then
    [ "$(find "$install_root" -print | wc -l | tr -d ' ')" = 1 ] || fail "Refusing to adopt an unowned nonempty directory"
fi
if [ -f "$state/owner" ]; then
    [ ! -L "$state/owner" ] && [ "$(cat "$state/owner")" = "ilium-posix-installer-1" ] || fail "Invalid ownership state"
    [ -f "$state/bin-dir" ] && [ ! -L "$state/bin-dir" ] && [ "$(cat "$state/bin-dir")" = "$bin_dir" ] || fail "Stable bin directory differs from recorded ownership"
fi
if [ "$uninstall" = yes ] && [ ! -f "$state/owner" ]; then
    printf 'ilium-install: stage=complete action=uninstall removed=0\n'
    exit 0
fi
mkdir -p "$install_root" "$install_root/versions" "$state" "$bin_dir" || fail "Cannot create user installation directories"
for directory in "$install_root" "$install_root/versions" "$state" "$bin_dir"; do
    [ -w "$directory" ] || fail "Installation directory is not writable"
done
stage=lock
mkdir "$install_root/.install-lock" 2>/dev/null || fail "Another installer owns the lock; do not remove it while that installer runs"
lock_owned=yes

remove_profile_block() {
    [ -f "$state/profile-path" ] || return 0
    profile=$(cat "$state/profile-path") || return 1
    valid_path "$profile" || return 2
    [ -f "$profile" ] && [ ! -L "$profile" ] || return 2
    offset=$(cat "$state/profile-offset") || return 1
    case "$offset" in ''|*[!0-9]*) return 2 ;; esac
    [ -f "$state/profile-block" ] && [ ! -L "$state/profile-block" ] || return 2
    length=$(wc -c < "$state/profile-block" | tr -d ' ')
    cp -p "$profile" "$work/profile-original" || return 1
    dd if="$work/profile-original" bs=1 skip="$offset" count="$length" 2>/dev/null > "$work/profile-fragment" || return 1
    cmp -s "$work/profile-fragment" "$state/profile-block" || return 2
    dd if="$work/profile-original" bs=1 count="$offset" 2>/dev/null > "$work/profile-clean" || return 1
    dd if="$work/profile-original" bs=1 skip="$((offset + length))" conv=notrunc 2>/dev/null >> "$work/profile-clean" || return 1
    # Prepare complete bytes on the profile's filesystem. A disk/write failure
    # must never truncate user configuration; concurrent edits stay untouched.
    profile_temp=$(mktemp "${profile%/*}/.ilium-profile.XXXXXXXX") || return 1
    cp -p "$profile" "$profile_temp" || return 1
    cat "$work/profile-clean" > "$profile_temp" || return 1
    cmp -s "$profile" "$work/profile-original" || return 2
    mv "$profile_temp" "$profile" || return 1
    profile_temp=
    rm -f "$state/profile-path" "$state/profile-offset" "$state/profile-block"
}

cleanup() {
    result=$?
    trap - 0 HUP INT TERM
    if [ "$result" -ne 0 ] && [ "$diagnosed" = no ]; then diagnostic "Filesystem operation or interruption failed during this stage"; fi
    if [ "$committed" = no ]; then
        # A signal can arrive after mv completed but before the next assignment.
        # Its private source disappears only after atomic profile publication.
        if [ "$profile_publishing" = yes ] && [ "$profile_added" = no ] && [ -n "$profile_temp" ] && [ ! -e "$profile_temp" ] && [ ! -L "$profile_temp" ]; then profile_added=yes; fi
        if [ "$profile_added" = yes ]; then remove_profile_block || printf 'ilium-install: warning=Owned profile block could not be rolled back; preserved for inspection.\n' >&2; fi
        if [ "$profile_metadata" = yes ] && [ "$profile_added" = no ]; then
            for profile_record in profile-path profile-offset profile-block; do
                if [ -f "$state/$profile_record" ] && [ ! -L "$state/$profile_record" ]; then rm -f "$state/$profile_record"; fi
            done
        fi
        # A receipt is prepared before its directory is published. If that
        # atomic directory rename fails, remove only this transaction's receipt.
        if [ "$new_receipt" = yes ] && [ ! -e "$install_root/versions/$version" ] && [ ! -L "$install_root/versions/$version" ] &&
            [ ! -L "$state/version-$version" ] && cmp -s "$work/version-receipt" "$state/version-$version"; then rm -f "$state/version-$version"; fi
        if [ "$new_client" = yes ] && cmp -s "$bin_dir/ilium" "$state/launcher-ilium"; then rm -f "$bin_dir/ilium" "$state/launcher-ilium"; fi
        if [ "$new_server" = yes ] && cmp -s "$bin_dir/ilium-server" "$state/launcher-ilium-server"; then rm -f "$bin_dir/ilium-server" "$state/launcher-ilium-server"; fi
    fi
    if [ -n "$work" ] && [ -d "$work" ] && [ ! -L "$work" ]; then
        # Only this mktemp-created, private staging tree is recursively removed.
        rm -rf "$work"
    fi
    if [ -n "$launcher_temp" ]; then rm -f "$launcher_temp"; fi
    if [ -n "$repair_temp" ]; then rm -f "$repair_temp"; fi
    if [ -n "$profile_temp" ]; then rm -f "$profile_temp"; fi
    if [ -n "$receipt_temp" ]; then rm -f "$receipt_temp"; fi
    if [ -n "$previous_temp" ]; then rm -f "$previous_temp" || printf 'ilium-install: warning=Preserved inaccessible private previous-state stage.\n' >&2; fi
    if [ "$lock_owned" = yes ]; then rm -f "$install_root/.install-lock/process"; rmdir "$install_root/.install-lock" || :; fi
    exit "$result"
}
trap cleanup 0
trap 'exit 1' HUP INT TERM
printf '%s\n' "$$" > "$install_root/.install-lock/process" || fail "Cannot record lock owner"
# The transaction's prior pointer is read and validated only while holding the
# installer lock. A waiting invocation must retain the immediately prior pair,
# not a snapshot captured before another installer completed.
stage=ownership
if [ -e "$install_root/current" ] || [ -L "$install_root/current" ]; then
    [ -f "$install_root/current" ] && [ ! -L "$install_root/current" ] || fail "Current pointer is not a regular file"
    newline='
'
    pointer=$(cat "$install_root/current" | od -A n -v -t u1 | LC_ALL=C awk '{for(i=1;i<=NF;i++){if($i==0)exit 1;printf "%c",$i}}' && printf '.') || fail "Invalid or unreadable current version pointer"
    case "$pointer" in *"$newline.") previous=${pointer%"$newline."} ;; *) fail "Invalid current version pointer" ;; esac
    valid_version "$previous" || fail "Invalid current version pointer"
    [ -f "$state/version-$previous" ] && [ ! -L "$state/version-$previous" ] || fail "Current pointer does not refer to an installer-owned version"
    if [ -f "$install_root/versions/$previous/bin/ilium" ] && [ -x "$install_root/versions/$previous/bin/ilium" ] && [ ! -L "$install_root/versions/$previous/bin/ilium" ] &&
        [ -f "$install_root/versions/$previous/bin/ilium-server" ] && [ -x "$install_root/versions/$previous/bin/ilium-server" ] && [ ! -L "$install_root/versions/$previous/bin/ilium-server" ]; then prior_active=yes; fi
fi
stage=prerequisites
require_hash
stage=staging
work=$(mktemp -d "$install_root/.stage.XXXXXXXX") || fail "Cannot create private staging directory"
chmod 700 "$work" || fail "Cannot secure staging directory"
if [ ! -f "$state/owner" ]; then
    printf 'ilium-posix-installer-1\n' > "$state/owner" || fail "Cannot record installation ownership"
    printf '%s\n' "$bin_dir" > "$state/bin-dir" || fail "Cannot record launcher directory"
fi

version_owned() {
    owned_version=$1
    valid_version "$owned_version" || return 1
    owned_directory=$install_root/versions/$owned_version
    receipt=$state/version-$owned_version
    [ -d "$owned_directory/bin" ] && [ ! -L "$owned_directory" ] && [ ! -L "$owned_directory/bin" ] && [ -f "$receipt" ] && [ ! -L "$receipt" ] || return 1
    awk '{if(NF!=2 || length($1)!=64 || $1!~/^[0-9a-f]+$/ || $2!~/^[A-Za-z0-9][A-Za-z0-9._-]*$/ || seen[$2]++){bad=1;exit 1}} END {bundle=seen["ilium-animation-helper"]+seen["beach-1.0.0.iliumanim"]+seen["carpet-1.0.0.iliumanim"]; if(bad || !seen["ilium"] || !seen["ilium-server"] || !seen["VERSION"] || !seen["THIRD-PARTY.txt"] || (bundle!=0 && bundle!=3))exit 1}' "$receipt" || return 1
    while IFS=' ' read -r _ member; do printf '%s\n' "$owned_directory/bin/$member"; done < "$receipt" > "$work/owned-paths"
    find "$owned_directory" -print > "$work/actual-paths" || return 1
    while IFS= read -r actual_path; do
        [ "$actual_path" != "$owned_directory" ] && [ "$actual_path" != "$owned_directory/bin" ] || continue
        grep -xF "$actual_path" "$work/owned-paths" >/dev/null || return 1
    done < "$work/actual-paths"
    while IFS=' ' read -r expected member; do
        case "$member" in ''|*[!A-Za-z0-9._-]*|*..*) return 1 ;; esac
        if [ "${2:-}" = allow-missing ] && [ ! -e "$owned_directory/bin/$member" ] && [ ! -L "$owned_directory/bin/$member" ]; then continue; fi
        [ -f "$owned_directory/bin/$member" ] && [ ! -L "$owned_directory/bin/$member" ] || return 1
        [ "$(hash_file "$owned_directory/bin/$member")" = "$expected" ] || return 1
        case "$member" in
            ilium|ilium-server|ilium-animation-helper) [ -x "$owned_directory/bin/$member" ] || return 1 ;;
            beach-1.0.0.iliumanim) [ "$expected" = 4b47934f4285ae426f680929b59af7151f4ac2e73ad41292872cfccd516cda30 ] || return 1 ;;
            carpet-1.0.0.iliumanim) [ "$expected" = c4cfdbc6d088361e488e8a7544162cc19a55dd0fea1c8bb237ad467b029db870 ] || return 1 ;;
        esac
    done < "$receipt"
}

remove_owned_version() {
    removal_version=$1
    version_owned "$removal_version" || { printf 'ilium-install: warning=Preserved modified or unknown version %s\n' "$removal_version" >&2; return 0; }
    while IFS=' ' read -r _ member; do rm -f "$install_root/versions/$removal_version/bin/$member" || return 1; done < "$state/version-$removal_version"
    rmdir "$install_root/versions/$removal_version/bin" "$install_root/versions/$removal_version" || return 1
    rm -f "$state/version-$removal_version"
}

if [ "$uninstall" = yes ]; then
    stage=uninstall
    if remove_profile_block; then :
    else
        profile_status=$?
        [ "$profile_status" -eq 2 ] || fail "Cannot safely remove owned profile PATH block; prior pair preserved"
        printf 'ilium-install: warning=Preserved a modified profile block.\n' >&2
    fi
    # Current pair corruption does not grant ownership of the changed bytes.
    for executable in ilium ilium-server; do
        if [ -f "$state/launcher-$executable" ] && [ ! -L "$bin_dir/$executable" ] && cmp -s "$bin_dir/$executable" "$state/launcher-$executable"; then
            rm -f "$bin_dir/$executable" "$state/launcher-$executable" || fail "Cannot remove owned launcher"
        fi
    done
    for receipt in "$state"/version-*; do
        [ -f "$receipt" ] || continue
        removal_version=${receipt##*/version-}
        remove_owned_version "$removal_version" || fail "Cannot remove owned version files"
    done
    rm -f "$install_root/current" "$state/previous"
    committed=yes
    printf 'ilium-install: stage=complete action=uninstall unknown_content=preserved\n'
    exit 0
fi

# Launcher ownership is checked before any download or profile modification.
stage=ownership
for executable in ilium ilium-server; do
    if [ -e "$bin_dir/$executable" ] || [ -L "$bin_dir/$executable" ]; then
        if [ ! -f "$state/launcher-$executable" ] || [ -L "$bin_dir/$executable" ] || ! cmp -s "$bin_dir/$executable" "$state/launcher-$executable"; then fail "Refusing to overwrite an unowned or modified launcher"; fi
    fi
done
stage=space
available=$(df -Pk "$install_root" | awk 'NR>1 {value=$4} END {print value}') || fail "Cannot inspect free space"
case "$available" in ''|*[!0-9]*) fail "Invalid free-space report" ;; esac
[ "$available" -ge 65536 ] || fail "At least 64 MiB of free staging space is required"

stage=download
command -v curl >/dev/null 2>&1 || fail "curl with HTTPS support is required"
origin=https://github.com/arthurwolf/ilium/releases
if [ -n "${ILIUM_INSTALL_TEST_ORIGIN:-}" ]; then
    # This explicit fixture seam never permits HTTP, credentials or arbitrary
    # production hosts. Real downloads always retain curl's TLS enforcement.
    printf '%s\n' "$ILIUM_INSTALL_TEST_ORIGIN" | awk 'NR!=1 || !/^https:\/\/127\.0\.0\.1:[0-9]+\/releases$/ {bad=1} END {exit bad}' || fail "Test origin must be an HTTPS loopback fixture"
    origin=$ILIUM_INSTALL_TEST_ORIGIN
fi
download() {
    curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fLsS --connect-timeout 15 --max-time 300 -o "$2" "$1" 2> "$work/download-error" || fail "HTTPS download failed (network, rate limit, or missing release asset); no latest fallback"
}
if [ -z "$version" ]; then
    resolved=$(curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fLsS --connect-timeout 15 --max-time 300 -o /dev/null -w '%{url_effective}' "$origin/latest" 2> "$work/download-error") || fail "Cannot resolve latest stable release"
    case "$resolved" in "$origin/tag/v"*) version=${resolved#"$origin/tag/v"} ;; *) fail "Latest release redirect is not the expected tagged origin" ;; esac
    valid_version "$version" || fail "Latest release tag is not a safe version"
    requested_version=$version
fi
download "$origin/download/v$version/$archive_name" "$work/$archive_name"
download "$origin/download/v$version/SHA256SUMS" "$work/SHA256SUMS"
stage=checksum
expected=$(awk -v selected="$archive_name" -v inventory="$checksum_archives" '
    BEGIN {count=split(inventory,names," ");for(i=1;i<=count;i++)required[names[i]]=1}
    {if (length($0)!=66+length($2) || length($1)!=64 || $1 !~ /^[0-9a-f]+$/ || substr($0,65,2)!="  " || !required[$2] || seen[$2]++) bad=1; if ($2==selected) {hash=$1; matches++}}
    END {if (bad || matches!=1 || NR!=count) exit 1; print hash}' "$work/SHA256SUMS") || fail "Malformed, duplicate or missing checksum record"
actual=$(hash_file "$work/$archive_name") || fail "SHA-256 tool failed"
[ "$actual" = "$expected" ] || fail "Archive SHA-256 mismatch"

stage=archive
gzip -dc "$work/$archive_name" > "$work/archive.tar" 2> "$work/archive-error" || fail "Malformed gzip archive or exhausted staging space"
archive_bytes=$(wc -c < "$work/archive.tar" | tr -d ' ')
[ "$archive_bytes" -le 2000000000 ] && [ "$archive_bytes" -ge 1024 ] && [ "$((archive_bytes % 512))" -eq 0 ] || fail "Unbounded or truncated tar archive"
prefix=${archive_name%.tar.gz}
block=0
member_count=0
total_size=0
: > "$work/members"
# Raw USTAR validation rejects PAX/GNU hidden headers before tar can interpret
# them. No links, extensions, device nodes, alternate prefixes or binary sizes.
while :; do
    dd if="$work/archive.tar" bs=512 skip="$block" count=1 2>/dev/null > "$work/header" || fail "Cannot read tar header"
    header=$(od -A n -v -t u1 "$work/header" | awk '
        {for(i=1;i<=NF;i++) b[++n]=$i}
        function text(start,len, i,s,zero) {s="";zero=0;for(i=start;i<start+len;i++){if(b[i]==0){zero=1;continue} if(zero) invalid=1; s=s sprintf("%c",b[i])} return s}
        function octal(start,len, i,s,v) {s="";for(i=start;i<start+len;i++)s=s sprintf("%c",b[i]==0?32:b[i]);gsub(/^ +| +$/,"",s);if(s!~/^[0-7]+$/){invalid=1;return 0}v=0;for(i=1;i<=length(s);i++)v=v*8+substr(s,i,1);return v}
        END {if(n!=512)exit 1;sum=0;for(i=1;i<=512;i++)sum+=b[i];if(sum==0){print "END";exit}check=octal(149,8);computed=0;for(i=1;i<=512;i++)computed+=(i>=149&&i<=156)?32:b[i];
          name=text(1,100);mode=octal(101,8);size=octal(125,12);type=b[157];
          if(invalid || computed!=check || text(258,6)!="ustar" || text(264,2)!="00" || text(346,155)!="" || (type!=48&&type!=0&&type!=53) || text(158,100)!="" || name!~/^[A-Za-z0-9][A-Za-z0-9._\/-]*$/)exit 1;
          printf "%s|%d|%d|%d\n",name,type,size,mode}') || fail "Unsafe or invalid USTAR header"
    if [ "$header" = END ]; then
        [ "$((archive_bytes / 512 - block))" -ge 2 ] || fail "Missing tar end markers"
        dd if="$work/archive.tar" bs=512 skip="$block" 2>/dev/null | od -A n -v -t u1 | awk '{for(i=1;i<=NF;i++)if($i!=0)exit 1}' || fail "Hidden data after tar terminator"
        break
    fi
    IFS='|' read -r member member_type member_size member_mode <<EOF
$header
EOF
    member_count=$((member_count + 1))
    [ "$member_count" -le 256 ] || fail "Archive contains too many members"
    if [ "$member_count" -eq 1 ]; then
        [ "${member%/}" = "$prefix" ] && [ "$member_type" = 53 ] && [ "$member_size" -eq 0 ] || fail "Archive root differs from target"
    else
        [ "$member_type" != 53 ] || fail "Nested archive directories are prohibited"
        case "$member" in "$prefix/"*) basename=${member#"$prefix/"} ;; *) fail "Archive path escapes expected root" ;; esac
        case "$basename" in ''|*/*|*..*) fail "Traversal or invalid archive member" ;; esac
        case "$basename" in
            ilium|ilium-server|ilium-animation-helper) [ "$member_mode" = 493 ] || fail "Executable mode is not 0755" ;;
            VERSION|THIRD-PARTY.txt|beach-1.0.0.iliumanim|carpet-1.0.0.iliumanim) [ "$member_mode" = 420 ] || fail "Data member mode is not 0644" ;;
            lib*.so|lib*.so.*) [ "$target_os" = linux ] && [ "$member_mode" = 420 ] || fail "Unexpected runtime library" ;;
            lib*.dylib) [ "$target_os" = macos ] && [ "$member_mode" = 420 ] || fail "Unexpected runtime library" ;;
            *) fail "Unexpected archive member" ;;
        esac
        printf '%s\n' "$basename" >> "$work/members"
    fi
    total_size=$((total_size + member_size))
    [ "$total_size" -le 2000000000 ] || fail "Archive member size exceeds bound"
    block=$((block + 1 + (member_size + 511) / 512))
    [ "$block" -lt "$((archive_bytes / 512))" ] || fail "Archive data is truncated"
done
LC_ALL=C sort -u "$work/members" > "$work/sorted-members"
cmp -s "$work/members" "$work/sorted-members" || fail "Duplicate or noncanonical archive member order"
for member in VERSION THIRD-PARTY.txt ilium ilium-server ilium-animation-helper beach-1.0.0.iliumanim carpet-1.0.0.iliumanim; do grep -x "$member" "$work/members" >/dev/null || fail "Archive is missing a required payload member"; done
mkdir "$work/extract" || fail "Cannot create private extraction directory"
(cd "$work/extract" && tar -xf ../archive.tar) 2> "$work/archive-error" || fail "Archive extraction failed"
candidate=$work/extract/$prefix
for member in ilium ilium-server ilium-animation-helper; do [ -f "$candidate/$member" ] && [ ! -L "$candidate/$member" ] && [ -x "$candidate/$member" ] || fail "Extracted executable payload is incomplete"; done
[ "$(hash_file "$candidate/beach-1.0.0.iliumanim")" = 4b47934f4285ae426f680929b59af7151f4ac2e73ad41292872cfccd516cda30 ] || fail "Official beach animation differs from compiled release identity"
[ "$(hash_file "$candidate/carpet-1.0.0.iliumanim")" = c4cfdbc6d088361e488e8a7544162cc19a55dd0fea1c8bb237ad467b029db870 ] || fail "Official carpet animation differs from compiled release identity"
printf '%s\n' "$version" > "$work/expected-version"
cmp -s "$candidate/VERSION" "$work/expected-version" || fail "Archive VERSION differs from requested release"
while IFS= read -r member; do printf '%s %s\n' "$(hash_file "$candidate/$member")" "$member"; done < "$work/members" > "$work/version-receipt"

stage=ownership
if [ -e "$install_root/versions/$version" ]; then
    if ! version_owned "$version" allow-missing || ! cmp -s "$work/version-receipt" "$state/version-$version"; then fail "Existing version differs from immutable verified archive; preserved"; fi
    while IFS= read -r member; do
        if [ ! -e "$install_root/versions/$version/bin/$member" ]; then
            repair_temp=$(mktemp "$install_root/versions/$version/bin/.ilium-repair.XXXXXXXX") || fail "Cannot stage missing owned member"
            cp -p "$candidate/$member" "$repair_temp" || fail "Cannot repair missing owned member"
            [ ! -e "$install_root/versions/$version/bin/$member" ] && [ ! -L "$install_root/versions/$version/bin/$member" ] || fail "Owned member changed during repair; preserved"
            mv "$repair_temp" "$install_root/versions/$version/bin/$member" || fail "Cannot atomically publish repaired member"
            repair_temp=
        fi
    done < "$work/members"
else
    [ ! -e "$state/version-$version" ] && [ ! -L "$state/version-$version" ] || fail "Unpublished version ownership state already exists; preserved"
    receipt_temp=$(mktemp "$state/.ilium-version.XXXXXXXX") || fail "Cannot stage version ownership"
    cp "$work/version-receipt" "$receipt_temp" || fail "Cannot record version ownership"
    cmp -s "$work/version-receipt" "$receipt_temp" || fail "Staged version ownership differs from verified receipt"
    mkdir "$work/version" || fail "Cannot stage complete version"
    mv "$candidate" "$work/version/bin" || fail "Cannot stage matched binary pair"
    new_receipt=yes
    mv "$receipt_temp" "$state/version-$version" || fail "Cannot publish version ownership"
    receipt_temp=
    mv "$work/version" "$install_root/versions/$version" || fail "Cannot install complete version"
    new_receipt=no
fi

stage=launchers
for executable in ilium ilium-server; do
    {
        printf '#!/bin/sh\n# ilium installer-owned launcher\nset -eu\ninstall_root='
        quote "$install_root"
        printf '\n'
        cat <<'LAUNCHER'
[ -f "$install_root/current" ] && [ ! -L "$install_root/current" ] || exit 1
[ -d "$install_root" ] && [ ! -L "$install_root" ] && [ -d "$install_root/versions" ] && [ ! -L "$install_root/versions" ] || exit 1
newline='
'
pointer=$(cat "$install_root/current" | od -A n -v -t u1 | LC_ALL=C awk '{for(i=1;i<=NF;i++){if($i==0)exit 1;printf "%c",$i}}' && printf '.') || { printf 'ilium: invalid installed version pointer\n' >&2; exit 1; }
case "$pointer" in *"$newline.") version=${pointer%"$newline."} ;; *) printf 'ilium: invalid installed version pointer\n' >&2; exit 1 ;; esac
printf '%s\n' "$version" | LC_ALL=C awk 'NR != 1 {bad=1} !/^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?$/ {bad=1} END {exit (bad || NR != 1)}' || { printf 'ilium: invalid installed version pointer\n' >&2; exit 1; }
directory=$install_root/versions/$version/bin
[ -d "$directory" ] && [ ! -L "$directory" ] && [ ! -L "$install_root/versions/$version" ] || exit 1
for binary in ilium ilium-server; do [ -f "$directory/$binary" ] && [ -x "$directory/$binary" ] && [ ! -L "$directory/$binary" ] || { printf 'ilium: installed binary pair is incomplete\n' >&2; exit 1; }; done
LAUNCHER
        printf 'exec "$directory/%s" "$@"\n' "$executable"
    } > "$work/launcher-$executable" || fail "Cannot write stable launcher"
    chmod 755 "$work/launcher-$executable" || fail "Cannot mark stable launcher executable"
    if [ -e "$bin_dir/$executable" ]; then
        cmp -s "$work/launcher-$executable" "$bin_dir/$executable" || fail "Existing launcher has a different installation contract"
    else
        cp "$work/launcher-$executable" "$state/launcher-$executable" || fail "Cannot record launcher ownership"
        launcher_temp=$(mktemp "$bin_dir/.ilium-launcher.XXXXXXXX") || fail "Cannot create private launcher stage"
        cp "$work/launcher-$executable" "$launcher_temp" || fail "Cannot stage stable launcher"
        chmod 755 "$launcher_temp" || fail "Cannot secure stable launcher"
        if [ -e "$bin_dir/$executable" ] || [ -L "$bin_dir/$executable" ]; then fail "Stable launcher destination changed during installation; preserved"; fi
        mv "$launcher_temp" "$bin_dir/$executable" || fail "Cannot install stable launcher"
        launcher_temp=
        case "$executable" in ilium) new_client=yes ;; ilium-server) new_server=yes ;; esac
    fi
done

stage=profile
if [ "$modify_path" = yes ] && [ ! -f "$state/profile-path" ]; then
    case "${SHELL:-/bin/sh}" in
        */bash) profile=$user_home/.bashrc ;;
        */zsh) profile=${ZDOTDIR:-$user_home}/.zshrc ;;
        */sh|*/dash|*/ksh) profile=$user_home/.profile ;;
        *) profile= ;;
    esac
    if [ -n "$profile" ]; then
        valid_path "$profile" && [ ! -L "$profile" ] || fail "Shell profile path is unsafe"
        [ ! -e "$profile" ] || [ -f "$profile" ] || fail "Shell profile is not a regular file"
        case ":${PATH:-}:" in *":$bin_dir:"*) needs_profile=no ;; *) needs_profile=yes ;; esac
        if [ -f "$profile" ] && grep -F "$bin_dir" "$profile" >/dev/null; then needs_profile=no; fi
        if [ "$needs_profile" = yes ]; then
            [ ! -e "$profile" ] || [ -w "$profile" ] || fail "Shell profile is not writable"
            offset=0
            profile_existed=no
            if [ -f "$profile" ]; then
                profile_existed=yes
                cp -p "$profile" "$work/profile-before" || fail "Cannot snapshot shell profile"
                offset=$(wc -c < "$work/profile-before" | tr -d ' ')
            fi
            {
                printf '\n# >>> ilium installer PATH >>>\ncase ":$PATH:" in *:'
                quote "$bin_dir"
                printf ':*) ;; *) export PATH='
                quote "$bin_dir"
                printf ':"$PATH" ;; esac\n# <<< ilium installer PATH <<<\n'
            } > "$work/profile-block" || fail "Cannot stage owned profile block"
            profile_temp=$(mktemp "${profile%/*}/.ilium-profile.XXXXXXXX") || fail "Cannot stage shell profile"
            if [ "$profile_existed" = yes ]; then cp -p "$work/profile-before" "$profile_temp" || fail "Cannot preserve shell profile bytes and mode"; fi
            cat "$work/profile-block" >> "$profile_temp" || fail "Shell profile PATH staging was rejected"
            printf '%s\n' "$profile" > "$work/profile-path" || fail "Cannot stage profile ownership path"
            printf '%s\n' "$offset" > "$work/profile-offset" || fail "Cannot stage profile ownership offset"
            for profile_record in profile-path profile-offset profile-block; do
                [ ! -e "$state/$profile_record" ] && [ ! -L "$state/$profile_record" ] || fail "Unknown profile ownership state already exists; preserved"
            done
            profile_metadata=yes
            for profile_record in profile-path profile-offset profile-block; do
                cp "$work/$profile_record" "$state/$profile_record" || fail "Cannot record owned profile metadata"
            done
            if [ "$profile_existed" = yes ]; then
                if [ ! -f "$profile" ] || [ -L "$profile" ] || ! cmp -s "$profile" "$work/profile-before"; then fail "Shell profile changed concurrently; preserved"; fi
            else
                [ ! -e "$profile" ] && [ ! -L "$profile" ] || fail "Shell profile appeared concurrently; preserved"
            fi
            profile_publishing=yes
            mv "$profile_temp" "$profile" || fail "Atomic shell profile publication failed; original bytes preserved"
            profile_added=yes
            profile_publishing=no
            profile_temp=
        fi
    fi
fi

stage=switch
version_owned "$version" || fail "New version pair is incomplete"
printf '%s\n' "$version" > "$work/current" || fail "Cannot stage version pointer"
mv "$work/current" "$install_root/current" || fail "Atomic pointer switch failed; previous pair remains active"
committed=yes
# Following maintenance cannot invalidate a successful pair activation.
persist_previous() {
    if [ -e "$state/previous" ] || [ -L "$state/previous" ]; then
        [ -f "$state/previous" ] && [ ! -L "$state/previous" ] && [ -w "$state/previous" ] || return 1
    fi
    previous_temp=$(mktemp "$state/.ilium-previous.XXXXXXXX") || return 1
    printf '%s\n' "$previous" > "$previous_temp" || return 1
    mv "$previous_temp" "$state/previous" || return 1
    previous_temp=
}
prune_versions=yes
retained_previous=$previous
if [ -n "$previous" ] && [ "$previous" != "$version" ]; then
    if ! persist_previous; then
        prune_versions=no
        printf 'ilium-install: warning=Could not record previous version; old versions preserved.\n' >&2
    fi
elif [ "$previous" = "$version" ]; then
    # A same-version retry has no distinct lock-protected prior pointer. An old
    # history record may have survived a failed write, so it cannot justify
    # deleting another version. Normal upgrades already perform their pruning.
    prune_versions=no
fi
if [ "$prune_versions" = yes ]; then
    for receipt in "$state"/version-*; do
        [ -f "$receipt" ] || continue
        removal_version=${receipt##*/version-}
        [ "$removal_version" != "$version" ] && [ "$removal_version" != "$retained_previous" ] || continue
        remove_owned_version "$removal_version" || printf 'ilium-install: warning=Could not prune older version %s; preserved.\n' "$removal_version" >&2
    done
fi
printf 'ilium-install: stage=complete version=%s target=%s matched_pair=verified\n' "$version" "$target"
case ":${PATH:-}:" in
    *":$bin_dir:"*) ;;
    *) printf 'For this shell, run: export PATH='; quote "$bin_dir"; printf ':"$PATH"\n' ;;
esac
