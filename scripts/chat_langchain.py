import sys
import json
import os
from langchain_openai import ChatOpenAI
from langchain_core.messages import HumanMessage, SystemMessage, AIMessage
from langchain_core.prompts import ChatPromptTemplate, MessagesPlaceholder
from langchain_core.output_parsers import StrOutputParser

def load_knowledge_base():
    """Load technical documentation to make Ami an expert on Arumanis system."""
    docs = [
        "docs/frontend/FEATURES.md",
        "docs/backend/DATABASE.md",
        "docs/backend/WILAYAH.md"
    ]
    kb_content = ""
    for doc_path in docs:
        try:
            full_path = os.path.join(os.getcwd(), doc_path)
            if os.path.exists(full_path):
                with open(full_path, 'r', encoding='utf-8') as f:
                    kb_content += f"\n\n--- DOKUMEN: {doc_path} ---\n"
                    kb_content += f.read()[:5000] 
            else:
                sys.stderr.write(f"Warning: Document not found: {full_path}\n")
        except Exception as e:
            sys.stderr.write(f"Error loading {doc_path}: {str(e)}\n")
            continue
    return kb_content

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

        # Load Knowledge Base
        kb = load_knowledge_base()

        # 1. Initialize LLM
        llm = ChatOpenAI(
            api_key=api_key,
            model=model,
            base_url="https://openrouter.ai/api/v1",
            temperature=0.1, # Even lower for expert accuracy
        )
        
        # 2. Define System Prompt Template
        system_prompt = """Anda adalah 'Ami', asisten AI SUPER EXPERT untuk aplikasi Arumanis (Sistem Informasi Pekerjaan Umum).
Anda memiliki pengetahuan mendalam tentang DATA proyek dan TEKNIS aplikasi Arumanis.

PENGETAHUAN SISTEM (MANUAL):
{knowledge_base}

KONTEKS DATA SAAT INI (REAL-TIME):
{context}

TUGAS ANDA:
1. Menjawab pertanyaan tentang data proyek (Pekerjaan, Kontrak, Progres, dll).
2. Menjelaskan fitur-fitur Arumanis jika ditanya cara penggunaan sistem.
3. Memberikan insight analisis data (progres, anggaran, wilayah).

ATURAN BISNIS (LOGIC INTERNAL):
- PERHITUNGAN PROGRES: Progres fisik dihitung berdasarkan bobot setiap item pekerjaan dan realisasi volumenya terhadap target volume.
- DATA MINGGUAN: Progres diakumulasi secara mingguan (Weekly Data).
- HAK AKSES: Data Pekerjaan difilter berdasarkan peran user. Admin melihat semua, Pengawas melihat yang di-assign via NIP atau tabel relasi user_pekerjaan.
- CHECKLIST: Pekerjaan dianggap lengkap administrasi jika `isChecklistComplete` bernilai true.

PETA RELASI (DATABASE):
- KEGIATAN (Induk): Berisi Pagu, Sumber Dana, dan Tahun Anggaran.
- PEKERJAAN: Anak dari Kegiatan (kegiatan_id). Punya lokasi (Kecamatan/Desa) dan data Progress.
- KONTRAK: Menghubungkan PEKERJAAN dengan PENYEDIA. Berisi Nilai Kontrak dan No. SPK.
- PROGRESS: Berisi detail bobot dan volume mingguan proyek.
- PENGAWAS: Terhubung ke PEKERJAAN melalui NIP Pengawas/Pendamping.

KRITERIA JAWABAN:
- WAJIB gunakan TABEL MARKDOWN untuk data list > 2.
- Jika ada URL foto/link, tampilkan dengan format Markdown.
- Bahasa: Indonesia yang cerdas, membantu, dan profesional.
- Jika data/info tidak ada, sarankan fitur yang relevan di Arumanis berdasarkan PENGETAHUAN SISTEM.
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
