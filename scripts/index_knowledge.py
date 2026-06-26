import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from rag_retriever import _chunk_title, tokenize


PROJECT_ROOT = Path(__file__).resolve().parent.parent
DOCS_ROOT = PROJECT_ROOT / 'docs'
INDEX_PATH = PROJECT_ROOT / 'storage' / 'ai' / 'knowledge_index.json'
MAX_CHUNK_CHARS = 1800


def split_markdown(text: str) -> list[str]:
    raw_chunks = [
        chunk.strip()
        for chunk in re.split(r"\n(?=#{1,6}\s)|\n{2,}", text)
        if chunk.strip()
    ]

    normalized: list[str] = []
    for chunk in raw_chunks:
        if len(chunk) <= MAX_CHUNK_CHARS:
            normalized.append(chunk)
            continue

        paragraphs = [p.strip() for p in chunk.split('\n\n') if p.strip()]
        buffer = ''
        for paragraph in paragraphs:
            candidate = f"{buffer}\n\n{paragraph}".strip() if buffer else paragraph
            if len(candidate) <= MAX_CHUNK_CHARS:
                buffer = candidate
                continue

            if buffer:
                normalized.append(buffer)
            buffer = paragraph

        if buffer:
            normalized.append(buffer)

    return normalized


def build_index() -> dict:
    if not DOCS_ROOT.exists():
        print(f"No docs directory found at {DOCS_ROOT}")
        return {
            'version': 2,
            'built_at': datetime.now(timezone.utc).isoformat(),
            'chunk_count': 0,
            'avg_doc_len': 0,
            'chunks': [],
            'inverted': {},
            'doc_freq': {},
        }

    chunks: list[dict] = []
    inverted: dict[str, list[int]] = {}
    doc_freq: dict[str, int] = {}
    total_len = 0

    for path in sorted(DOCS_ROOT.rglob('*.md')):
        try:
            text = path.read_text(encoding='utf-8')
        except UnicodeDecodeError:
            text = path.read_text(encoding='utf-8', errors='ignore')

        source = str(path.relative_to(PROJECT_ROOT))

        for content in split_markdown(text):
            chunk_id = len(chunks)
            tokens = tokenize(content)
            term_freqs: dict[str, int] = {}

            for token in tokens:
                term_freqs[token] = term_freqs.get(token, 0) + 1
                if term_freqs[token] == 1:
                    inverted.setdefault(token, []).append(chunk_id)

            for token in term_freqs:
                doc_freq[token] = doc_freq.get(token, 0) + 1

            doc_len = len(tokens)
            total_len += doc_len

            chunks.append({
                'id': chunk_id,
                'source': source,
                'title': _chunk_title(content),
                'content': content,
                'term_freqs': term_freqs,
                'length': doc_len,
            })

    count = len(chunks) or 1

    return {
        'version': 2,
        'built_at': datetime.now(timezone.utc).isoformat(),
        'chunk_count': len(chunks),
        'avg_doc_len': total_len / count,
        'chunks': chunks,
        'inverted': inverted,
        'doc_freq': doc_freq,
    }


def index_docs() -> None:
    payload = build_index()
    INDEX_PATH.parent.mkdir(parents=True, exist_ok=True)
    INDEX_PATH.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2),
        encoding='utf-8',
    )
    print(
        f"Indexed {payload['chunk_count']} chunks "
        f"({len(payload['doc_freq'])} terms) to {INDEX_PATH}"
    )


if __name__ == '__main__':
    index_docs()