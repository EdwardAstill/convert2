#!/usr/bin/env bash
# Re-populate papers/ with the test corpus referenced by tests/golden.rs.
# Run once after a fresh clone. Total download is ~30 MB.

set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/papers"
golden="$out/golden"
mkdir -p "$golden"

echo "→ Downloading arXiv ML papers into $out ..."
cd "$out"
for name_url in \
    "attention.pdf https://arxiv.org/pdf/1706.03762" \
    "resnet.pdf https://arxiv.org/pdf/1512.03385" \
    "clip.pdf https://arxiv.org/pdf/2103.00020" \
    "gpt3.pdf https://arxiv.org/pdf/2005.14165" \
    "bert.pdf https://arxiv.org/pdf/1810.04805" \
    "math-number-theory.pdf https://arxiv.org/pdf/2203.12412" \
    "physics-hep.pdf https://arxiv.org/pdf/2401.14447" \
    "survey-llm.pdf https://arxiv.org/pdf/2303.18223"
do
    name="${name_url% *}"
    url="${name_url#* }"
    [[ -f "$name" ]] && continue
    echo "  $name ← $url"
    curl -sSL -o "$name" "$url"
done

echo "→ OpenDataLoader fixtures should be copied manually from:"
echo "    https://github.com/opendataloader-project/opendataloader-pdf/tree/main/samples/pdf"
echo "  into $golden/"
echo "✓ Done."
