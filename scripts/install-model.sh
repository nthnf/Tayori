#!/usr/bin/env bash
set -Eeuo pipefail

APP_NAME="tayori"
HF_REPO_URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main"
DEFAULT_MODEL="large-v3-turbo-q5_0"

# Tuple format:
# idx|model_name|sha1|disk_size
MODELS=(
  "0|tiny|bd577a113a864445d4c299885e0cb97d4ba92b5f|75 MiB"
  "1|tiny.en|c78c86eb1a8faa21b369bcd33207cc90d64ae9df|75 MiB"
  "2|base|465707469ff3a37a2b9b8d8f89f2f99de7299dac|142 MiB"
  "3|base.en|137c40403d78fd54d454da0f9bd998f78703390c|142 MiB"
  "4|small|55356645c2b361a969dfd0ef2c5a50d530afd8d5|466 MiB"
  "5|small.en|db8a495a91d927739e50b3fc1cc4c6b8f6c2d022|466 MiB"
  "6|small.en-tdrz|b6c6e7e89af1a35c08e6de56b66ca6a02a2fdfa1|465 MiB"
  "7|medium|fd9727b6e1217c2f614f9b698455c4ffd82463b4|1.5 GiB"
  "8|medium.en|8c30f0e44ce9560643ebd10bbe50cd20eafd3723|1.5 GiB"
  "9|large-v1|b1caaf735c4cc1429223d5a74f0f4d0b9b59a299|2.9 GiB"
  "10|large-v2|0f4c8e34f21cf1a914c59d8b3ce882345ad349d6|2.9 GiB"
  "11|large-v2-q5_0|00e39f2196344e901b3a2bd5814807a769bd1630|1.1 GiB"
  "12|large-v3|ad82bf6a9043ceed055076d0fd39f5f186ff8062|2.9 GiB"
  "13|large-v3-q5_0|e6e2ed78495d403bef4b7cff42ef4aaadcfea8de|1.1 GiB"
  "14|large-v3-turbo|4af2b29d7ec73d781377bfd1758ca957a807e941|1.5 GiB"
  "15|large-v3-turbo-q5_0|e050f7970618a659205450ad97eb95a18d69c9ee|547 MiB"
)

MODEL=""
MODEL_IDX=""
FORCE=0
QUIET=0

usage() {
  cat <<EOF
Usage:
  $0 list
  $0 download [--model MODEL | --idx IDX] [--force] [--quiet]

Examples:
  $0 list
  $0 download
  $0 download --model large-v3-turbo-q5_0
  $0 download --idx 15 --quiet

Default model:
  ${DEFAULT_MODEL}

Default install path:
  \${XDG_DATA_HOME:-\$HOME/.local/share}/${APP_NAME}/models/whisper
EOF
}

log() {
  if [[ "$QUIET" -eq 0 ]]; then
    echo "$@" >&2
  fi
}

die() {
  echo "error: $*" >&2
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

model_dir() {
  local data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
  printf '%s/%s/models/whisper\n' "$data_home" "$APP_NAME"
}

model_filename() {
  local model_name="$1"
  printf 'ggml-%s.bin\n' "$model_name"
}

model_path() {
  local model_name="$1"
  printf '%s/%s\n' "$(model_dir)" "$(model_filename "$model_name")"
}

find_model_by_name() {
  local wanted="$1"

  for row in "${MODELS[@]}"; do
    IFS='|' read -r idx name sha size <<<"$row"

    if [[ "$name" == "$wanted" ]]; then
      printf '%s|%s|%s|%s\n' "$idx" "$name" "$sha" "$size"
      return 0
    fi
  done

  return 1
}

find_model_by_idx() {
  local wanted="$1"

  for row in "${MODELS[@]}"; do
    IFS='|' read -r idx name sha size <<<"$row"

    if [[ "$idx" == "$wanted" ]]; then
      printf '%s|%s|%s|%s\n' "$idx" "$name" "$sha" "$size"
      return 0
    fi
  done

  return 1
}

resolve_model() {
  if [[ -n "$MODEL_IDX" ]]; then
    find_model_by_idx "$MODEL_IDX" || die "unknown model idx: $MODEL_IDX"
  else
    local name="${MODEL:-$DEFAULT_MODEL}"
    find_model_by_name "$name" || die "unknown model name: $name"
  fi
}

sha1_of_file() {
  local path="$1"

  if command -v sha1sum >/dev/null 2>&1; then
    sha1sum "$path" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 1 "$path" | awk '{print $1}'
  else
    die "missing sha1sum or shasum"
  fi
}

verify_downloaded_model() {
  local path="$1"
  local expected_sha="$2"

  [[ -f "$path" ]] || die "model file not found after download: $path"

  log "Verifying SHA1..."

  local actual_sha
  actual_sha="$(sha1_of_file "$path")"

  if [[ "$actual_sha" != "$expected_sha" ]]; then
    rm -f "$path"

    die "checksum mismatch; deleted invalid model
expected: $expected_sha
actual:   $actual_sha"
  fi

  log "Checksum OK."
}

download_model() {
  require_cmd curl

  local model_name="$1"
  local expected_sha="$2"
  local size="$3"

  local dir
  dir="$(model_dir)"

  local filename
  filename="$(model_filename "$model_name")"

  local path
  path="$(model_path "$model_name")"

  local url="$HF_REPO_URL/$filename"

  mkdir -p "$dir"

  if [[ -f "$path" && "$FORCE" -eq 0 ]]; then
    log "Model already exists: $path"
    verify_downloaded_model "$path" "$expected_sha"
    printf '%s\n' "$path"
    return 0
  fi

  if [[ "$FORCE" -eq 1 ]]; then
    rm -f "$path"
  fi

  log "Downloading Whisper model:"
  log "  model: $model_name"
  log "  size:  $size"
  log "  url:   $url"
  log "  path:  $path"

  curl \
    --location \
    --fail \
    --continue-at - \
    --output "$path" \
    "$url"

  verify_downloaded_model "$path" "$expected_sha"

  # stdout intentionally returns the final model path for Rust callers.
  printf '%s\n' "$path"
}

list_models() {
  printf 'idx\tmodel_name\tsha1\tdisk_size\n'

  for row in "${MODELS[@]}"; do
    IFS='|' read -r idx name sha size <<<"$row"
    printf '%s\t%s\t%s\t%s\n' "$idx" "$name" "$sha" "$size"
  done
}

COMMAND="${1:-}"
[[ -n "$COMMAND" ]] || {
  usage
  exit 1
}
shift || true

while [[ $# -gt 0 ]]; do
  case "$1" in
  --model)
    MODEL="${2:-}"
    [[ -n "$MODEL" ]] || die "--model requires a value"
    shift 2
    ;;
  --idx)
    MODEL_IDX="${2:-}"
    [[ -n "$MODEL_IDX" ]] || die "--idx requires a value"
    shift 2
    ;;
  --force)
    FORCE=1
    shift
    ;;
  --quiet)
    QUIET=1
    shift
    ;;
  -h | --help)
    usage
    exit 0
    ;;
  *)
    die "unknown arg: $1"
    ;;
  esac
done

if [[ -n "$MODEL" && -n "$MODEL_IDX" ]]; then
  die "use either --model or --idx, not both"
fi

case "$COMMAND" in
list)
  list_models
  ;;

download)
  row="$(resolve_model)"
  IFS='|' read -r idx name sha size <<<"$row"
  download_model "$name" "$sha" "$size"
  ;;

*)
  usage
  exit 1
  ;;
esac
