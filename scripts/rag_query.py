import sys
import json
import os
import re

sys.stdout.reconfigure(encoding='utf-8')

try:
    input_data = json.loads(sys.stdin.read() or '{}')
except json.JSONDecodeError:
    print(json.dumps({'content': '', 'chunks': [], 'error': 'Invalid JSON input'}, ensure_ascii=False))
    sys.exit(0)

query = input_data.get('query', '')
n_results = input_data.get('n_results', 5)

try:
    n_results = max(1, min(int(n_results), 20))
except (TypeError, ValueError):
    n_results = 5

try:
    import chromadb
    from chromadb.utils import embedding_functions
except ImportError:
    print(json.dumps({'content': '', 'chunks': [], 'error': 'chromadb not installed'}, ensure_ascii=False))
    sys.exit(0)

base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
db_path = os.path.join(base_dir, 'storage', 'ai', 'vector_db')

try:
    client = chromadb.PersistentClient(path=db_path)
    ef = embedding_functions.DefaultEmbeddingFunction()

    # Prioritaskan koleksi vector 384-dim (langchain_v2), fallback ke langchain
    try:
        collection = client.get_collection('langchain_v2', embedding_function=ef)
    except Exception:
        collection = client.get_collection('langchain')
except Exception as e:
    print(json.dumps({'content': '', 'chunks': [], 'error': f'Koleksi tidak ditemukan: {e}'}, ensure_ascii=False))
    sys.exit(0)

if not query.strip():
    print(json.dumps({'content': 'Pengetahuan sistem masih kosong.', 'chunks': [], 'error': None}, ensure_ascii=False))
    sys.exit(0)

chunks = []

# 1. Semantic vector search via ChromaDB
try:
    res = collection.query(query_texts=[query], n_results=n_results)
    if res and res.get('documents') and res['documents'][0]:
        for doc, meta in zip(res['documents'][0], res['metadatas'][0] if res.get('metadatas') else []):
            src = (meta or {}).get('source', 'unknown')
            chunks.append(f"--- {src} ---\n{doc}")
except Exception:
    pass

# 2. Fallback keyword search jika vector query kosong/gagal
if not chunks:
    STOP = set('dan di ke dari yang ini itu dengan untuk pada adalah bisa tidak akan sudah telah saya kami anda kamu apa siapa kapan dimana bagaimana kenapa the is are this that with for and not can will has have been was were ada atau juga agar oleh sebagai dalam antara'.split())

    tokens = [t for t in re.findall(r'\w+', query.lower()) if len(t) > 2 and t not in STOP]
    if not tokens:
        tokens = re.findall(r'\w+', query.lower())

    try:
        data = collection.get(include=['documents', 'metadatas'])
        scored = []
        for i, doc in enumerate(data.get('documents') or []):
            low = doc.lower()
            score = sum(low.count(t) for t in tokens)
            if score > 0:
                meta = (data.get('metadatas') or [{}])[i] or {}
                scored.append((score, meta.get('source', 'unknown'), doc))

        scored.sort(key=lambda x: -x[0])
        top = scored[:n_results]
        chunks = [f"--- {src} ---\n{doc}" for _, src, doc in top]
    except Exception as e:
        print(json.dumps({'content': '', 'chunks': [], 'error': str(e)}, ensure_ascii=False))
        sys.exit(0)

content = '\n\n'.join(chunks) if chunks else 'Pengetahuan sistem masih kosong.'
print(json.dumps({'content': content, 'chunks': chunks, 'error': None}, ensure_ascii=False))
