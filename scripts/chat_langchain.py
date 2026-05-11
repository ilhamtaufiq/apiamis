import sys
import json
import os
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
        
        # Search for top 5 most relevant chunks
        docs = vectorstore.similarity_search(query, k=5)
        
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
- Gunakan TABEL MARKDOWN untuk list data.
- **CHART SUPPORT**: Jika user bertanya soal tren, perbandingan, atau statistik yang cocok dijadikan grafik, berikan blok kode khusus di akhir jawaban dengan format:
  ```json
  { "type": "chart", "chart_type": "bar|pie|line", "data": [...] }
  ```
- **DEEP LINKING**: Jika user ingin melaporkan masalah, berikan link ke form tiket dengan parameter pekerjaan_id (misal: `/tiket/create?pekerjaan_id=123`).

STRATEGI ANALISA DATA:
1. Jika ditanya 'Laporan Pagi' atau 'Ringkasan Eksekutif', cari paket yang progresnya < 10% atau yang memiliki tiket 'Open' dan berikan ringkasan kritis.
2. Jika ditanya soal JUMLAH atau TOTAL, WAJIB ambil data dari 'RINGKASAN STATISTIK DATA'.
3. Selalu bandingkan data statistik dengan daftar detail.

STRATEGI JIKA DATA TIDAK DITEMUKAN / PERTANYAAN TIDAK DIMENGERTI:
1. Mohon Maaf secara sopan (gunakan emoji 🤖 atau 🧩).
2. Tampilkan data alternatif yang mungkin relevan (jika ada).
3. Berikan "💡 CONTOH PERTANYAAN YANG BISA ANDA COBA":
   - "Ami, tampilkan laporan pagi hari ini."
   - "Tampilkan grafik perbandingan progres per kecamatan."
   - "Ada tiket masalah apa yang belum selesai?"
4. Berikan tips singkat.

KONTEKS WILAYAH: Fokus pada desa/kecamatan di Kabupaten Cianjur.
CONTOH NADA BICARA:
"Sampurasun bos! Wilujeng enjing. 📌 Berdasarkan data real-time, ada 3 paket yang butuh perhatian karena progresnya masih 0% meskipun SPK sudah turun..."

PENGETAHUAN SISTEM (MANUAL):
{knowledge_base}

KONTEKS DATA SAAT INI (REAL-TIME):
{context}
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
            "usage": {}
        }))

    except Exception as e:
        print(json.dumps({
            "success": False,
            "error": str(e)
        }))

if __name__ == "__main__":
    run_chat()
