"""Lightweight CLI for PHP to fetch RAG context without loading LangChain."""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from rag_retriever import retrieve_relevant_docs


def main() -> None:
    raw = sys.stdin.read()
    payload = json.loads(raw) if raw else {}
    query = payload.get('query', '')
    print(json.dumps({'content': retrieve_relevant_docs(query)}, ensure_ascii=False))


if __name__ == '__main__':
    main()