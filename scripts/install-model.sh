#!/usr/bin/env bash
set -Eeuo pipefail

APP_NAME="tayori"
HF_REPO_URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main"
DEFAULT_MODEL="large-v3-turbo-q5_0"

# Tuple format:
# idx|model_name|sha1|disk_size
MODELS=(
  "0|tiny|bd577a113a864445d4c299885e0cb97d4ba92b5f|75 MiB"
  "1|tiny-q5_1|2827a03e495b1ed3048ef28a6a4620537db4ee51|31 MiB"
  "2|tiny-q8_0|19e8118f6652a650569f5a949d962154e01571d9|42 MiB"
  "3|tiny.en|c78c86eb1a8faa21b369bcd33207cc90d64ae9df|75 MiB"
  "4|tiny.en-q5_1|3fb92ec865cbbc769f08137f22470d6b66e071b6|31 MiB"
  "5|tiny.en-q8_0|802d6668e7d411123e672abe4cb6c18f12306abb|42 MiB"

  "6|base|465707469ff3a37a2b9b8d8f89f2f99de7299dac|142 MiB"
  "7|base-q5_1|a3733eda680ef76256db5fc5dd9de8629e62c5e7|57 MiB"
  "8|base-q8_0|7bb89bb49ed6955013b166f1b6a6c04584a20fbe|78 MiB"
  "9|base.en|137c40403d78fd54d454da0f9bd998f78703390c|142 MiB"
  "10|base.en-q5_1|d26d7ce5a1b6e57bea5d0431b9c20ae49423c94a|57 MiB"
  "11|base.en-q8_0|bb1574182e9b924452bf0cd1510ac034d323e948|78 MiB"

  "12|small|55356645c2b361a969dfd0ef2c5a50d530afd8d5|466 MiB"
  "13|small-q5_1|6fe57ddcfdd1c6b07cdcc73aaf620810ce5fc771|181 MiB"
  "14|small-q8_0|bcad8a2083f4e53d648d586b7dbc0cd673d8afad|252 MiB"
  "15|small.en|db8a495a91d927739e50b3fc1cc4c6b8f6c2d022|466 MiB"
  "16|small.en-q5_1|20f54878d608f94e4a8ee3ae56016571d47cba34|181 MiB"
  "17|small.en-q8_0|9d75ff4ccfa0a8217870d7405cf8cef0a5579852|252 MiB"
  "18|small.en-tdrz|b6c6e7e89af1a35c08e6de56b66ca6a02a2fdfa1|465 MiB"

  "19|medium|fd9727b6e1217c2f614f9b698455c4ffd82463b4|1.5 GiB"
  "20|medium-q5_0|7718d4c1ec62ca96998f058114db98236937490e|514 MiB"
  "21|medium-q8_0|e66645948aff4bebbec71b3485c576f3d63af5d6|785 MiB"
  "22|medium.en|8c30f0e44ce9560643ebd10bbe50cd20eafd3723|1.5 GiB"
  "23|medium.en-q5_0|bb3b5281bddd61605d6fc76bc5b92d8f20284c3b|514 MiB"
  "24|medium.en-q8_0|b1cf48c12c807e14881f634fb7b6c6ca867f6b38|785 MiB"

  "25|large-v1|b1caaf735c4cc1429223d5a74f0f4d0b9b59a299|2.9 GiB"
  "26|large-v2|0f4c8e34f21cf1a914c59d8b3ce882345ad349d6|2.9 GiB"
  "27|large-v2-q5_0|00e39f2196344e901b3a2bd5814807a769bd1630|1.1 GiB"
  "28|large-v2-q8_0|da97d6ca8f8ffbeeb5fd147f79010eeea194ba38|1.5 GiB"
  "29|large-v3|ad82bf6a9043ceed055076d0fd39f5f186ff8062|2.9 GiB"
  "30|large-v3-q5_0|e6e2ed78495d403bef4b7cff42ef4aaadcfea8de|1.1 GiB"
  "31|large-v3-turbo|4af2b29d7ec73d781377bfd1758ca957a807e941|1.5 GiB"
  "32|large-v3-turbo-q5_0|e050f7970618a659205450ad97eb95a18d69c9ee|547 MiB"
  "33|large-v3-turbo-q8_0|01bf15bedffe9f39d65c1b6ff9b687ea91f59e0e|834 MiB"
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
