import json
import re
from pathlib import Path


PROJECT_ROOT = Path(__file__).resolve().parent.parent
DOCS_ROOT = PROJECT_ROOT / 'docs'
INDEX_PATH = PROJECT_ROOT / 'storage' / 'ai' / 'knowledge_index.json'


def split_markdown(text):
    return [
        chunk.strip()
        for chunk in re.split(r"\n(?=#{1,6}\s)|\n{2,}", text)
        if chunk.strip()
    ]


def index_docs():
    if not DOCS_ROOT.exists():
        print(f"No docs directory found at {DOCS_ROOT}")
        return

    entries = []

    for path in DOCS_ROOT.rglob('*.md'):
        try:
            text = path.read_text(encoding='utf-8')
        except UnicodeDecodeError:
            text = path.read_text(encoding='utf-8', errors='ignore')

        for chunk in split_markdown(text):
            entries.append({
                'source': str(path.relative_to(PROJECT_ROOT)),
                'content': chunk,
            })

    INDEX_PATH.parent.mkdir(parents=True, exist_ok=True)
    INDEX_PATH.write_text(
        json.dumps(entries, ensure_ascii=False, indent=2),
        encoding='utf-8',
    )

    print(f"Indexed {len(entries)} chunks to {INDEX_PATH}")


if __name__ == "__main__":
    index_docs()
