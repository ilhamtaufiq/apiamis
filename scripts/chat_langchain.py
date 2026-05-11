import sys
import json
import os
from pathlib import Path

# FIX: Set HOME environment variable for ChromaDB/Pathlib in server environments
# This must happen before importing langchain_chroma
# Determine project root (2 levels up from /scripts)
project_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ai_storage = os.path.join(project_root, 'storage', 'ai')
os.makedirs(ai_storage, exist_ok=True)

# FORCE set environment variables for ChromaDB/Pathlib
os.environ['HOME'] = ai_storage
os.environ['USERPROFILE'] = ai_storage
os.environ['KMP_DUPLICATE_LIB_OK'] = 'TRUE' # Bonus fix for OpenMP issues on Windows

from langchain_openai import ChatOpenAI
from langchain_core.messages import HumanMessage, SystemMessage, AIMessage
from langchain_core.prompts import ChatPromptTemplate, MessagesPlaceholder
from langchain_core.output_parsers import StrOutputParser

from langchain_chroma import Chroma
from langchain_openai import OpenAIEmbeddings

def get_relevant_docs(query, api_key):
    """Retrieve relevant document chunks from ChromaDB."""
    persist_directory = 'storage/ai/vector_db'
    
    if not os.path.exists(persist_directory):
        return "Pengetahuan sistem belum diindeks."

    try:
        embeddings = OpenAIEmbeddings(
            api_key=api_key,
            base_url="https://openrouter.ai/api/v1"
        )
        vectorstore = Chroma(
            persist_directory=persist_directory,
            embedding_function=embeddings
        )
        
        # Search for top 20 most relevant chunks
        docs = vectorstore.similarity_search(query, k=20)
        
        content = ""
        for i, doc in enumerate(docs):
            source = doc.metadata.get('source', 'Unknown')
            content += f"\n\n--- SUMBER {i+1} ({source}) ---\n"
            content += doc.page_content
            
        return content
    except Exception as e:
        return f"Gagal mengambil dokumen: {str(e)}"

def run_chat():
    try:
        # Read from stdin
        input_raw = sys.stdin.read()
        if not input_raw:
            raise ValueError("No input received")
            
        input_data = json.loads(input_raw)
        
        api_key = input_data.get('api_key')
        model = input_data.get('model', 'openrouter/free')
        user_message = input_data.get('message')
        context = input_data.get('context', '')
        history_raw = input_data.get('history', [])
        
        if not api_key:
            raise ValueError("API Key is required")

        # Load Knowledge Base via Vector Retrieval
        kb = get_relevant_docs(user_message, api_key)

        # 1. Initialize LLM
        llm = ChatOpenAI(
            api_key=api_key,
            model=model,
            base_url="https://openrouter.ai/api/v1",
            temperature=0.1, # Even lower for expert accuracy
        )
        
        # 2. Define System Prompt Template
        system_prompt = """Anda adalah 'Ami', asisten AI SUPER EXPERT untuk aplikasi Arumanis (Sistem Informasi Bidang Air Minum dan Sanitasi - Kabupaten Cianjur).

GAYA BAHASA & PERSONA (SUPER MODE):
- Sapa user dengan bahasa Sunda yang sopan di awal (misal: "Sampurasun bos!", "Wilujeng enjing!").
- Gunakan Emoji yang relevan untuk mempercantik tampilan (📌, 💡, 🔍, 📊, 😊).
- **WAJIB TABEL**: Setiap menampilkan daftar paket/data lebih dari 1, GUNAKAN TABEL MARKDOWN yang rapi dengan header.
- **CHART SUPPORT**: Jika ada data statistik, berikan blok kode khusus di akhir jawaban:
  ```json
  {{ "type": "chart", "chart_type": "bar|pie|line", "data": [...] }}
  ```
- **DYNAMIC DEEP LINKING**: Jika merekomendasikan buat tiket, gunakan link `/tiket/create?pekerjaan_id=[ID_ASLI]`. Ganti `[ID_ASLI]` dengan kolom `id` yang ada di data context. JANGAN gunakan '123' atau 'XXX'.

STRATEGI ANALISA DATA:
1. Jika ditanya 'Laporan Pagi' atau 'Ringkasan Eksekutif', cari paket yang progresnya < 10% atau yang memiliki tiket 'Open' dan berikan ringkasan kritis.
2. **ANALISA ANOMALI**: Jika ada paket dengan kontrak aktif (SPK turun) tapi progres masih 0%, sebutkan itu sebagai ANOMALI KONTRAK. Sebutkan juga jika ada penyedia ganda yang mengerjakan banyak proyek sekaligus.
3. Selalu bandingkan data statistik dengan daftar detail.

STRATEGI JIKA DATA TIDAK DITEMUKAN:
1. Mohon Maaf secara sopan.
2. Tampilkan tips "💡 COBA TANYA SEPERTI INI".

KONTEKS WILAYAH: Fokus pada desa/kecamatan di Kabupaten Cianjur.
CONTOH NADA BICARA:
"Sampurasun bos! Wilujeng enjing. 📌 Berdasarkan data real-time, ada 3 paket yang butuh perhatian karena progresnya masih 0% meskipun SPK sudah turun..."

PENGETAHUAN SISTEM (MANUAL):
{knowledge_base}

KONTEKS DATA SAAT INI (REAL-TIME):
{context}
*(Instruksi Khusus: Jika user bertanya tentang jumlah total paket, nilai uang, atau statistik makro, WAJIB merujuk pada bagian 'RINGKASAN STATISTIK' di atas. Jangan menghitung manual dari daftar detail karena daftar tersebut sengaja dibatasi untuk efisiensi).*
"""

        prompt = ChatPromptTemplate.from_messages([
            ("system", system_prompt),
            MessagesPlaceholder(variable_name="history"),
            ("human", "{input}"),
        ])

        # 3. Format History
        formatted_history = []
        for msg in history_raw[-10:]:
            if msg.get('role') == 'user':
                formatted_history.append(HumanMessage(content=msg.get('content')))
            else:
                formatted_history.append(AIMessage(content=msg.get('content')))

        # 4. Create Chain
        chain = prompt | llm | StrOutputParser()

        # 5. Execute
        response_content = chain.invoke({
            "input": user_message,
            "context": context,
            "knowledge_base": kb,
            "history": formatted_history
        })
        
        # Output as JSON
        print(json.dumps({
            "success": True,
            "content": response_content,
            "model": model,
            "usage": {}
        }))

    except Exception as e:
        print(json.dumps({
            "success": False,
            "error": str(e)
        }))

if __name__ == "__main__":
    run_chat()
