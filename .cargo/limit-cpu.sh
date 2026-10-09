#!/bin/sh
# Restrict compiler descendants to one Cargo job, common child-tool thread pools,
# and one logical CPU so builds cannot spread across cores or SMT siblings.
set -eu
if [ "$(hostname)" != ubuntu-vm ]; then
    if [ "${1:-}" = "--cargo" ]; then
        shift
        exec /home/arthur/.local/bin/ni-build cargo "$@"
    fi
    printf '%s\n' 'Local compilation is disabled; use the ni-build Cargo dispatcher.' >&2
    exit 75
fi
if [ "${1:-}" = "--cargo" ]; then
    shift
    set -- nice -n 19 ionice -c 2 -n 7 env \
        CARGO_BUILD_JOBS=1 \
        CARGO_PROFILE_DEV_CODEGEN_UNITS=1 \
        CARGO_PROFILE_TEST_CODEGEN_UNITS=1 \
        CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
        CARGO_PROFILE_BENCH_CODEGEN_UNITS=1 \
        CARGO_PROFILE_DEV_BUILD_OVERRIDE_CODEGEN_UNITS=1 \
        CARGO_PROFILE_TEST_BUILD_OVERRIDE_CODEGEN_UNITS=1 \
        CARGO_PROFILE_RELEASE_BUILD_OVERRIDE_CODEGEN_UNITS=1 \
        CARGO_PROFILE_BENCH_BUILD_OVERRIDE_CODEGEN_UNITS=1 \
        CMAKE_BUILD_PARALLEL_LEVEL=1 \
        MAKEFLAGS=-j1 \
        NINJAFLAGS=-j1 \
        OMP_NUM_THREADS=1 \
        RAYON_NUM_THREADS=1 \
        OPENBLAS_NUM_THREADS=1 \
        MKL_NUM_THREADS=1 \
        RUST_TEST_THREADS=1 \
        cargo "$@"
fi
# CPU affinity is a local Linux policy; job and child-tool limits apply elsewhere.
if [ "$(uname -s)" != Linux ]; then
    exec "$@"
fi
# Pick one CPU already allowed to this process, including inside a container or
# cpuset. Using one logical CPU avoids both multi-core and SMT parallelism.
allowed_cpus=$(taskset --cpu-list --pid "$$" | sed 's/.*: //')
cpu=$(printf '%s\n' "$allowed_cpus" | sed 's/[-,].*//')
case "$cpu" in
    ''|*[!0-9]*)
        echo 'Cannot determine one allowed compilation CPU' >&2
        exit 1
        ;;
esac
if [ -z "$allowed_cpus" ]; then
    echo 'Cannot determine the allowed compilation CPU set' >&2
    exit 1
fi
exec taskset --cpu-list "$cpu" "$@"
