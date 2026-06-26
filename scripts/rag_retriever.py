"""Fast BM25 retrieval over pre-built knowledge index."""

from __future__ import annotations

import json
import math
import re
from functools import lru_cache
from pathlib import Path
from typing import Iterable

PROJECT_ROOT = Path(__file__).resolve().parent.parent
INDEX_PATH = PROJECT_ROOT / 'storage' / 'ai' / 'knowledge_index.json'

STOP_WORDS = {
    'apa', 'apaan', 'bagaimana', 'bisa', 'boleh', 'dan', 'dari', 'di', 'dong', 'ini',
    'itu', 'juga', 'kah', 'kapan', 'ke', 'mau', 'oleh', 'pada', 'saja', 'saya', 'siapa',
    'sih', 'sudah', 'supaya', 'tampilkan', 'tanpa', 'telah', 'tentang', 'tersebut', 'tidak',
    'tolong', 'untuk', 'yang', 'adalah', 'ada', 'agar', 'atau', 'dengan', 'akan', 'the',
    'and', 'for', 'how', 'what', 'when', 'where', 'who', 'why',
}

DOMAIN_SYNONYMS: dict[str, list[str]] = {
    'pekerjaan': ['paket', 'proyek', 'kegiatan'],
    'paket': ['pekerjaan', 'proyek'],
    'kontrak': ['spk', 'kontraktor'],
    'spk': ['kontrak'],
    'penyedia': ['kontraktor', 'vendor', 'rekanan'],
    'sanitasi': ['spam', 'mck', 'jamban'],
    'spam': ['sanitasi', 'mck'],
    'air': ['pam', 'minum', 'pdam'],
    'progress': ['progres', 'capaian', 'realisasi'],
    'progres': ['progress', 'capaian'],
    'tiket': ['laporan', 'keluhan', 'issue'],
    'foto': ['dokumentasi', 'gambar'],
    'penerima': ['beneficiary', 'jiwa', 'kk'],
    'output': ['komponen', 'volume'],
    'desa': ['kelurahan'],
    'kecamatan': ['kec'],
    'pagu': ['anggaran', 'budget'],
    'role': ['peran', 'hak', 'akses'],
    'permission': ['izin', 'hak'],
}

BM25_K1 = 1.2
BM25_B = 0.75
HYBRID_BM25_WEIGHT = 0.55
HYBRID_TFIDF_WEIGHT = 0.45
TOP_K = 6
MAX_CONTEXT_CHARS = 4500
MIN_TERM_LEN = 3


def _normalize(text: str) -> str:
    return re.sub(r'\s+', ' ', text.lower()).strip()


def tokenize(text: str, *, min_len: int = MIN_TERM_LEN) -> list[str]:
    tokens = re.findall(r'[a-z0-9][a-z0-9_-]*', _normalize(text))
    filtered: list[str] = []

    for token in tokens:
        if len(token) < min_len or token in STOP_WORDS:
            continue
        filtered.append(token)

    return filtered


def expand_query_terms(terms: Iterable[str]) -> list[str]:
    expanded: list[str] = []
    seen: set[str] = set()

    for term in terms:
        if term in seen:
            continue
        seen.add(term)
        expanded.append(term)

        for synonym in DOMAIN_SYNONYMS.get(term, []):
            if synonym not in seen:
                seen.add(synonym)
                expanded.append(synonym)

    return expanded


def _chunk_title(content: str) -> str:
    for line in content.splitlines():
        line = line.strip()
        if line.startswith('#'):
            return re.sub(r'^#+\s*', '', line).strip()
    return ''


def _bm25_score(
    term_freqs: dict[str, int],
    doc_len: int,
    avg_doc_len: float,
    query_terms: list[str],
    doc_freq: dict[str, int],
    total_docs: int,
) -> float:
    if doc_len <= 0 or avg_doc_len <= 0 or total_docs <= 0:
        return 0.0

    score = 0.0
    for term in query_terms:
        tf = term_freqs.get(term, 0)
        if tf <= 0:
            continue

        df = doc_freq.get(term, 0)
        idf = math.log(1 + (total_docs - df + 0.5) / (df + 0.5))
        numerator = tf * (BM25_K1 + 1)
        denominator = tf + BM25_K1 * (1 - BM25_B + BM25_B * (doc_len / avg_doc_len))
        score += idf * (numerator / denominator)

    return score


def _build_query_tfidf(query_terms: list[str], idf_map: dict[str, float]) -> tuple[dict[str, float], float]:
    tf_counts: dict[str, int] = {}
    for term in query_terms:
        tf_counts[term] = tf_counts.get(term, 0) + 1

    weights: dict[str, float] = {}
    for term, tf in tf_counts.items():
        weights[term] = (1 + math.log(tf)) * idf_map.get(term, 0.0)

    norm = math.sqrt(sum(value * value for value in weights.values()))
    return weights, norm


def _cosine_sparse(
    left: dict[str, float],
    left_norm: float,
    right: dict[str, float],
    right_norm: float,
) -> float:
    if left_norm <= 0 or right_norm <= 0:
        return 0.0

    score = 0.0
    for term, left_weight in left.items():
        right_weight = right.get(term)
        if right_weight:
            score += left_weight * right_weight

    return score / (left_norm * right_norm)


def _attach_tfidf(index: dict) -> dict:
    if index.get('idf'):
        return index

    total_docs = int(index.get('chunk_count') or len(index.get('chunks', [])) or 1)
    doc_freq = index.get('doc_freq', {})
    idf = {
        term: math.log((total_docs + 1) / (df + 1)) + 1
        for term, df in doc_freq.items()
    }

    for chunk in index.get('chunks', []):
        tfidf: dict[str, float] = {}
        for term, tf in chunk.get('term_freqs', {}).items():
            tfidf[term] = (1 + math.log(tf)) * idf.get(term, 0.0)
        chunk['tfidf'] = tfidf
        chunk['tfidf_norm'] = math.sqrt(sum(value * value for value in tfidf.values()))

    index['idf'] = idf
    index['version'] = max(int(index.get('version') or 2), 3)
    return index


@lru_cache(maxsize=1)
def _load_index() -> dict | None:
    if not INDEX_PATH.exists():
        return None

    try:
        payload = json.loads(INDEX_PATH.read_text(encoding='utf-8'))
    except Exception:
        return None

    if isinstance(payload, list):
        return _upgrade_legacy_index(payload)

    if isinstance(payload, dict) and int(payload.get('version') or 0) >= 2:
        return _attach_tfidf(payload)

    return None


def _upgrade_legacy_index(entries: list[dict]) -> dict:
    chunks: list[dict] = []
    inverted: dict[str, list[int]] = {}
    doc_freq: dict[str, int] = {}
    total_len = 0

    for idx, entry in enumerate(entries):
        content = entry.get('content', '')
        tokens = tokenize(content)
        term_freqs: dict[str, int] = {}

        for token in tokens:
            term_freqs[token] = term_freqs.get(token, 0) + 1
            if term_freqs[token] == 1:
                inverted.setdefault(token, []).append(idx)

        for token in term_freqs:
            doc_freq[token] = doc_freq.get(token, 0) + 1

        doc_len = len(tokens)
        total_len += doc_len
        chunks.append({
            'id': idx,
            'source': entry.get('source', 'unknown'),
            'title': _chunk_title(content),
            'content': content,
            'term_freqs': term_freqs,
            'length': doc_len,
        })

    count = len(chunks) or 1
    return _attach_tfidf({
        'version': 3,
        'chunk_count': len(chunks),
        'avg_doc_len': total_len / count,
        'chunks': chunks,
        'inverted': inverted,
        'doc_freq': doc_freq,
    })


def retrieve_relevant_docs(query: str) -> str:
    index = _load_index()
    if index is None:
        return 'Pengetahuan sistem belum tersedia (jalankan `php artisan chat:index-knowledge`).'

    query_terms = expand_query_terms(tokenize(query))
    if not query_terms:
        return ''

    chunks = index['chunks']
    inverted = index['inverted']
    doc_freq = index['doc_freq']
    avg_doc_len = float(index.get('avg_doc_len') or 1)
    total_docs = int(index.get('chunk_count') or len(chunks) or 1)

    candidate_ids: set[int] = set()
    for term in query_terms:
        for chunk_id in inverted.get(term, []):
            candidate_ids.add(chunk_id)

    if not candidate_ids:
        return 'Tidak ada potongan dokumen yang relevan ditemukan.'

    idf_map = index.get('idf', {})
    query_tfidf, query_norm = _build_query_tfidf(query_terms, idf_map)

    scored: list[tuple[float, float, float, dict]] = []
    chunk_map = {chunk['id']: chunk for chunk in chunks}

    for chunk_id in candidate_ids:
        chunk = chunk_map.get(chunk_id)
        if not chunk:
            continue

        bm25 = _bm25_score(
            chunk.get('term_freqs', {}),
            int(chunk.get('length') or 0),
            avg_doc_len,
            query_terms,
            doc_freq,
            total_docs,
        )
        tfidf = _cosine_sparse(
            query_tfidf,
            query_norm,
            chunk.get('tfidf', {}),
            float(chunk.get('tfidf_norm') or 0),
        )
        if bm25 > 0 or tfidf > 0:
            scored.append((bm25, tfidf, 0.0, chunk))

    if not scored:
        return 'Tidak ada potongan dokumen yang relevan ditemukan.'

    max_bm25 = max(item[0] for item in scored) or 1.0
    hybrid_scored: list[tuple[float, dict]] = []
    for bm25, tfidf, _, chunk in scored:
        bm25_norm = bm25 / max_bm25
        hybrid = (HYBRID_BM25_WEIGHT * bm25_norm) + (HYBRID_TFIDF_WEIGHT * tfidf)
        hybrid_scored.append((hybrid, chunk))

    hybrid_scored.sort(key=lambda item: item[0], reverse=True)
    scored = hybrid_scored

    content = ''
    used_sources: set[str] = set()

    for rank, (score, chunk) in enumerate(scored[:TOP_K], start=1):
        source = chunk.get('source', 'unknown')
        title = chunk.get('title') or source
        body = chunk.get('content', '')
        snippet = body[:1200]

        block = (
            f"\n\n--- SUMBER {rank} ({source}) | skor={score:.2f} ---\n"
            f"Judul: {title}\n"
            f"{snippet}"
        )

        if len(content) + len(block) > MAX_CONTEXT_CHARS:
            break

        content += block
        used_sources.add(source)

    if not content:
        return 'Tidak ada potongan dokumen yang relevan ditemukan.'

    return content.strip()