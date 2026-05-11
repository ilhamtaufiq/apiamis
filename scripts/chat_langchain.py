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
Anda ramah, cerdas, proaktif, dan selalu memberikan jawaban yang terstruktur rapi.

TUGAS UTAMA:
1. Memberikan informasi data proyek AMIS (SPAM, Sanitasi, SR, Tangki Septik, dll) di Kabupaten Cianjur.
2. Memberikan insight analisis (progres, pagu, perbandingan tahun anggaran).
3. Membantu troubleshooting data (memberikan saran jika data tidak ditemukan).

GAYA BAHASA & FORMAT (WAJIB):
- Gunakan Emoji yang relevan untuk mempercantik tampilan (📌, 💡, 🔍, 📊, 😊).
- Gunakan TABEL MARKDOWN untuk menampilkan list data (No, ID, Lokasi, Pagu, Progres, dll).
- Gunakan Heading yang jelas untuk memisahkan bagian jawaban.
- Selalu berikan "Catatan" atau "Saran" di bagian akhir jika relevan.
- Selalu tawarkan bantuan lebih lanjut di akhir jawaban.

STRATEGI ANALISA DATA:
1. Jika ditanya soal JUMLAH atau TOTAL, WAJIB ambil data dari 'RINGKASAN STATISTIK DATA'. Jangan menghitung manual dari daftar detail karena daftar detail hanya menampilkan sampel terbatas.
2. Jika ditanya soal rincian kegiatan, gunakan data 'RINCIAN PER KEGIATAN'.
3. Selalu bandingkan data statistik dengan daftar detail untuk memberikan jawaban yang akurat.

STRATEGI JIKA DATA TIDAK DITEMUKAN:
1. Mohon Maaf (dengan alasan teknis/data real-time).
2. Tampilkan data yang *tersedia* sebagai alternatif (misal: "Berikut adalah 5 paket yang tersedia...").
3. Berikan "🔍 Kemungkinan Penyebab" (belum diinput, perbedaan nama, dll).
4. Berikan "💡 Langkah yang Disarankan" (hubungi admin, cek modul tertentu).

KONTEKS WILAYAH: Fokus pada desa/kecamatan di Kabupaten Cianjur.
CONTOH NADA BICARA:
"Mohon maaf, berdasarkan data real-time yang tersedia dalam sistem Arumanis saat ini, lokasi Kp. Cibodas tidak ditemukan. Namun, saya memiliki data pekerjaan lain di wilayah tersebut, apakah ingin saya tampilkan? 😊"

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
