import sys
import json
import os
from pathlib import Path
import re

# Determine project root (2 levels up from /scripts)
project_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ai_storage = os.path.join(project_root, 'storage', 'ai')
os.makedirs(ai_storage, exist_ok=True)

os.environ['HOME'] = ai_storage
os.environ['USERPROFILE'] = ai_storage
os.environ['KMP_DUPLICATE_LIB_OK'] = 'TRUE' # Bonus fix for OpenMP issues on Windows

from langchain_openai import ChatOpenAI
from langchain_core.messages import HumanMessage, SystemMessage, AIMessage, ToolMessage
from langchain_core.prompts import ChatPromptTemplate, MessagesPlaceholder

def get_relevant_docs(query, api_key):
    """Retrieve relevant Markdown chunks without heavyweight vector dependencies."""
    docs_root = Path(project_root) / 'docs'

    if not docs_root.exists():
        return "Pengetahuan sistem belum tersedia."

    query_terms = {
        term for term in re.findall(r"\w+", query.lower())
        if len(term) >= 4
    }

    matches = []

    for path in docs_root.rglob('*.md'):
        try:
            text = path.read_text(encoding='utf-8')
        except UnicodeDecodeError:
            text = path.read_text(encoding='utf-8', errors='ignore')

        chunks = [
            chunk.strip()
            for chunk in re.split(r"\n(?=#{1,6}\s)|\n{2,}", text)
            if chunk.strip()
        ]

        for chunk in chunks:
            lowered = chunk.lower()
            score = sum(1 for term in query_terms if term in lowered)
            if score > 0:
                matches.append((score, path.relative_to(project_root), chunk[:1500]))

    if not matches:
        return "Tidak ada potongan dokumen yang relevan ditemukan."

    matches.sort(key=lambda item: item[0], reverse=True)

    content = ""
    for i, (_, source, chunk) in enumerate(matches[:12]):
        content += f"\n\n--- SUMBER {i+1} ({source}) ---\n"
        content += chunk

    return content

def run_chat():
    try:
        # Read from stdin
        input_raw = sys.stdin.read()
        if not input_raw:
            raise ValueError("No input received")
            
        input_data = json.loads(input_raw)
        
        api_key = input_data.get('api_key')
        model = input_data.get('model', 'openrouter/free')
        base_url = input_data.get('base_url', 'https://openrouter.ai/api/v1')
        headers = input_data.get('headers', {})
        user_message = input_data.get('message')
        context = input_data.get('context', '')
        history_raw = input_data.get('history', [])
        tools_def = input_data.get('tools')
        tool_history = input_data.get('tool_history', [])
        
        if not api_key:
            raise ValueError("API Key is required")

        # Load Knowledge Base from local Markdown docs
        kb = get_relevant_docs(user_message, api_key)

        # 1. Initialize LLM
        llm = ChatOpenAI(
            api_key=api_key,
            model=model,
            base_url=base_url,
            default_headers=headers if headers else None,
            temperature=0.1,
        )

        # Bind tools if provided
        if tools_def:
            llm = llm.bind_tools(tools_def)
        
        # 2. Define System Prompt Template
        system_prompt = """Anda adalah 'Ami', asisten AI SUPER EXPERT untuk aplikasi Arumanis (Sistem Informasi Bidang Air Minum dan Sanitasi - Kabupaten Cianjur).

GAYA BAHASA & PERSONA (SUPER MODE):
- Sapa user dengan bahasa Sunda yang sopan di awal (misal: "Sampurasun bos!", "Wilujeng enjing!").
- Gunakan Emoji yang relevan (📌, 💡, 🔍, 📊, 😊).
- **WAJIB TABEL**: Setiap menampilkan daftar paket/data lebih dari 1, GUNAKAN TABEL MARKDOWN yang rapi.
- **CHART SUPPORT**: Jika ada data statistik, berikan blok kode khusus:
  ```json
  {{ "type": "chart", "chart_type": "bar|pie|line", "data": [...] }}
  ```

STRATEGI ANALISA DATA:
1. **GUNAKAN TOOLS** jika pertanyaan membutuhkan data aktual dari database.
2. **JANGAN MENEBAK** angka, status, atau daftar data. Gunakan tool yang paling spesifik.
3. Gunakan `search_projects` lalu `get_project_details` untuk detail paket.
4. Gunakan tool domain terkait bila user bertanya tentang kontrak, tiket, foto, output, penerima, atau penyedia.
5. Jika hasil pencarian ambigu, tampilkan kandidat yang relevan dan jelaskan filter yang dipakai.

KONTEKS WILAYAH: Fokus pada desa/kecamatan di Kabupaten Cianjur.

PENGETAHUAN SISTEM (RETRIEVED):
{knowledge_base}

KONTEKS DATA AWAL (STATIC):
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

        # Append tool call history
        if tool_history:
            for interaction in tool_history:
                ast_msg = interaction.get('assistant', {})
                # Use additional_kwargs for OpenAI-style tool_calls in history
                formatted_history.append(AIMessage(
                    content=ast_msg.get('content', ''),
                    additional_kwargs={'tool_calls': ast_msg.get('tool_calls', [])}
                ))
                for res in interaction.get('results', []):
                    formatted_history.append(ToolMessage(
                        tool_call_id=res['tool_call_id'],
                        content=res['content']
                    ))

        # 4. Create Chain (use direct LLM invoke to get tool_calls)
        chain_input = {
            "input": user_message,
            "context": context,
            "knowledge_base": kb,
            "history": formatted_history
        }
        
        full_prompt = prompt.format_messages(**chain_input)
        response = llm.invoke(full_prompt)
        
        # 5. Execute & Output
        output = {
            "success": True,
            "content": response.content,
            "model": model,
            "usage": response.response_metadata.get('token_usage', {})
        }

        if hasattr(response, 'additional_kwargs') and 'tool_calls' in response.additional_kwargs:
            output['tool_calls'] = response.additional_kwargs['tool_calls']
        elif hasattr(response, 'tool_calls') and response.tool_calls:
            # Normalize LangChain native tool_calls to OpenAI format for PHP compatibility
            normalized_calls = []
            for tc in response.tool_calls:
                normalized_calls.append({
                    'id': tc.get('id'),
                    'type': 'function',
                    'function': {
                        'name': tc.get('name'),
                        'arguments': json.dumps(tc.get('args')) if isinstance(tc.get('args'), dict) else tc.get('args')
                    }
                })
            output['tool_calls'] = normalized_calls

        print(json.dumps(output))

    except Exception as e:
        print(json.dumps({
            "success": False,
            "error": str(e)
        }))

if __name__ == "__main__":
    run_chat()

