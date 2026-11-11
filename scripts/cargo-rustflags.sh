append_cargo_rustflags() {
    local argument separator=$'\x1f'
    for argument in "$@"; do
        if [[ "$argument" == *"$separator"* ]]; then
            printf 'error: required Cargo flag contains encoded separator\n' >&2
            return 1
        fi
    done
    if [[ ${CARGO_ENCODED_RUSTFLAGS+x} ]]; then
        for argument in "$@"; do
            if [[ -n "$CARGO_ENCODED_RUSTFLAGS" ]]; then
                CARGO_ENCODED_RUSTFLAGS+="$separator"
            fi
            CARGO_ENCODED_RUSTFLAGS+="$argument"
        done
        export CARGO_ENCODED_RUSTFLAGS
    else
        for argument in "$@"; do
            if [[ "$argument" == *[[:space:]]* ]]; then
                printf 'error: required Cargo flag contains whitespace; set CARGO_ENCODED_RUSTFLAGS\n' >&2
                return 1
            fi
            RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }$argument"
        done
        export RUSTFLAGS
    fi
}
