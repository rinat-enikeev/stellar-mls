#!/usr/bin/env bash
# phase2-helper — wrapper around snarkjs for Onym Phase 2 rounds.
#
# Subcommands:
#   help
#   export-srs   --round N   --tier T   --out FILE
#                Convert the final Phase 1 SRS for a tier into a snarkjs
#                .ptau file.
#   init         --circuit FILE --ptau FILE --out FILE
#                Run `snarkjs groth16 setup` to produce the zkey_0000.
#   contribute   --input IN --output OUT --name STR
#                Run `snarkjs zkey contribute` and emit a receipt.json
#                alongside OUT with the zkey_hash + transcript digest.
#   verify       --circuit FILE --ptau FILE --zkey FILE
#                Run `snarkjs zkey verify` and exit non-zero on mismatch.
#   beacon       --input IN --output OUT --hash HEX --iterations N
#                Apply the public beacon contribution.
#   export-vk    --zkey FILE --out FILE
#                Emit the verification key JSON.

set -euo pipefail

HELP=$(cat <<'USAGE'
Usage:
  phase2-helper <subcommand> [options]

Subcommands:
  help
  init         --circuit FILE --ptau FILE --out FILE
  contribute   --input IN --output OUT --name STR [--entropy HEX]
  verify       --circuit FILE --ptau FILE --zkey FILE
  beacon       --input IN --output OUT --hash HEX --iterations N
  export-vk    --zkey FILE --out FILE

All file paths are resolved inside /work (mount your local folder as
-v "$(pwd):/work" when invoking `docker run`).

Input .ptau: the coordinator publishes a snarkjs-compatible Powers of Tau
file per tier at `/api/v1/phase2/ptau/<tier>.ptau` after Phase 1 freezes.
Download that file and mount it into /work before calling `init`.
USAGE
)

die() { echo "phase2-helper: $*" >&2; exit 1; }

sha256_of() { sha256sum "$1" | awk '{print $1}'; }

ensure_args() {
  local needed=("$@")
  for k in "${needed[@]}"; do
    if [ -z "${!k:-}" ]; then die "missing --${k//_/-}"; fi
  done
}

parse_flags() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --round)      ROUND="$2"; shift 2;;
      --tier)       TIER="$2"; shift 2;;
      --out)        OUT="$2"; shift 2;;
      --circuit)    CIRCUIT="$2"; shift 2;;
      --ptau)       PTAU="$2"; shift 2;;
      --input)      INPUT="$2"; shift 2;;
      --output)     OUTPUT="$2"; shift 2;;
      --name)       NAME="$2"; shift 2;;
      --entropy)    ENTROPY="$2"; shift 2;;
      --hash)       HASH="$2"; shift 2;;
      --iterations) ITERATIONS="$2"; shift 2;;
      --zkey)       ZKEY="$2"; shift 2;;
      *) die "unknown flag: $1";;
    esac
  done
}

cmd_init() {
  parse_flags "$@"
  ensure_args CIRCUIT PTAU OUT
  snarkjs groth16 setup "$CIRCUIT" "$PTAU" "$OUT"
  echo "wrote $OUT (sha256=$(sha256_of "$OUT"))"
}

cmd_contribute() {
  parse_flags "$@"
  ensure_args INPUT OUTPUT NAME
  # snarkjs asks for entropy interactively if not given; we require
  # --entropy or /dev/urandom to keep the run deterministic for the
  # caller.
  local entropy="${ENTROPY:-}"
  if [ -z "$entropy" ]; then
    entropy=$(head -c 64 /dev/urandom | xxd -p -c 128)
  fi
  snarkjs zkey contribute "$INPUT" "$OUTPUT" \
    --name="$NAME" \
    -e="$entropy" \
    --verbose | tee "${OUTPUT}.transcript.txt"

  local in_hash out_hash tr_hash
  in_hash=$(sha256_of "$INPUT")
  out_hash=$(sha256_of "$OUTPUT")
  tr_hash=$(sha256_of "${OUTPUT}.transcript.txt")

  cat > "${OUTPUT}.receipt.json" <<RECEIPT
{
  "phase": "phase2",
  "name": "$NAME",
  "input_zkey_sha256": "$in_hash",
  "output_zkey_sha256": "$out_hash",
  "transcript_sha256": "$tr_hash",
  "snarkjs_version": "$(snarkjs --version 2>/dev/null || echo unknown)",
  "created_at": $(date +%s)
}
RECEIPT
  echo "wrote $OUTPUT (sha256=$out_hash)"
  echo "wrote ${OUTPUT}.receipt.json"
}

cmd_verify() {
  parse_flags "$@"
  ensure_args CIRCUIT PTAU ZKEY
  snarkjs zkey verify "$CIRCUIT" "$PTAU" "$ZKEY"
}

cmd_beacon() {
  parse_flags "$@"
  ensure_args INPUT OUTPUT HASH ITERATIONS
  snarkjs zkey beacon "$INPUT" "$OUTPUT" "$HASH" "$ITERATIONS" \
    --name="phase2-public-beacon"
  echo "wrote $OUTPUT (sha256=$(sha256_of "$OUTPUT"))"
}

cmd_export_vk() {
  parse_flags "$@"
  ensure_args ZKEY OUT
  snarkjs zkey export verificationkey "$ZKEY" "$OUT"
  echo "wrote $OUT"
}

case "${1:-help}" in
  help|-h|--help) echo "$HELP";;
  init)        shift; cmd_init "$@";;
  contribute)  shift; cmd_contribute "$@";;
  verify)      shift; cmd_verify "$@";;
  beacon)      shift; cmd_beacon "$@";;
  export-vk)   shift; cmd_export_vk "$@";;
  *) die "unknown subcommand: $1 (try: phase2-helper help)";;
esac
