#!/usr/bin/env bash
# Merekam respon GET dari API Laravel yang berjalan sebagai fixture perilaku (fase 0.4).
#
# Hanya melakukan GET. Setiap respon disimpan ke folder keluaran setelah field
# sensitif dihapus (token, password, NIK, NIP, email, alamat, cookie).
#
# Pemakaian (dijalankan di komputer yang bisa menjangkau BASE_URL):
#   BASE_URL=https://apiamis.cianjur.space API_TOKEN=xxx ./record.sh
#   BASE_URL=... ./record.sh            # tanpa token: endpoint yang butuh login akan 401
#
# Keluaran: ./out/<nama>.json berisi status, content-type, dan body yang sudah dibersihkan.
set -euo pipefail

BASE_URL="${BASE_URL:?set BASE_URL, contoh https://apiamis.cianjur.space}"
BASE_URL="${BASE_URL%/}"
OUT_DIR="${OUT_DIR:-$(dirname "$0")/out}"
mkdir -p "$OUT_DIR"

# Endpoint yang direkam: nama_file|path
ENDPOINTS=(
  "up|/up"
  "health|/api/health"
  "kecamatan_index|/api/kecamatan"
  "desa_index|/api/desa"
  "kegiatan_index|/api/kegiatan"
  "pekerjaan_index|/api/pekerjaan"
  "not_found|/api/tidak-ada-sama-sekali"
  "api_docs_json|/docs/api-docs.json"
)

AUTH_HEADER=()
if [[ -n "${API_TOKEN:-}" ]]; then
  AUTH_HEADER=(-H "Authorization: Bearer ${API_TOKEN}")
fi

for entry in "${ENDPOINTS[@]}"; do
  name="${entry%%|*}"
  path="${entry#*|}"
  tmp="$(mktemp)"
  status=$(curl -sS -m 30 -o "$tmp" -w "%{http_code}" -H "Accept: application/json" \
    ${AUTH_HEADER[@]+"${AUTH_HEADER[@]}"} "${BASE_URL}${path}" || echo "000")
  ctype=$(file -b --mime-type "$tmp" 2>/dev/null || echo "unknown")

  python3 - "$tmp" "$OUT_DIR/$name.json" "$path" "$status" "$ctype" <<'PY'
import json, re, sys

src, dst, path, status, ctype = sys.argv[1:6]
SENSITIVE = {
    "token", "password", "remember_token", "nik", "nip", "email", "alamat",
    "encrypted_cookies", "access_token", "refresh_token", "google_id", "avatar", "nama_pptk",
}
SENSITIVE_HINT = re.compile(r"(token|password|secret|cookie|nik|nip|email|telepon|telp|phone|hp|alamat|address|npwp|ktp|whatsapp)", re.I)

PERSON_PARENTS = {"pengawas", "pendamping", "penerima"}

def scrub(v, parent=None):
    if isinstance(v, dict):
        out = {}
        for k, val in v.items():
            if k in SENSITIVE or SENSITIVE_HINT.search(k) or (parent in PERSON_PARENTS and k == "nama"):
                out[k] = "<redacted>"
            else:
                out[k] = scrub(val, k)
        return out
    if isinstance(v, list):
        return [scrub(x, parent) for x in v]
    return v

raw = open(src, encoding="utf-8", errors="replace").read()
try:
    parsed = json.loads(raw)
    body_kind = "json"
    # Respon exception Laravel (APP_DEBUG=true) memuat path server dan stack trace.
    # Jangan disimpan sebagai fixture.
    if isinstance(parsed, dict) and "exception" in parsed and "trace" in parsed:
        body = "<debug exception omitted>"
    else:
        body = scrub(parsed)
except ValueError:
    body = "<non-json body omitted>" if "<html" in raw[:200].lower() else raw[:2000]
    body_kind = "text"

record = {
    "path": path,
    "status": int(status) if status.isdigit() else status,
    "content_type": ctype,
    "body_kind": body_kind,
    "body": body,
}
with open(dst, "w", encoding="utf-8") as f:
    json.dump(record, f, ensure_ascii=False, indent=2, sort_keys=True)
    f.write("\n")
print(f"{path} -> {status} ({body_kind}) -> {dst}")
PY
  rm -f "$tmp"
done
