import sys
import json
import os
from pathlib import Path

scripts_dir = Path(__file__).resolve().parent
sys.path.insert(0, str(scripts_dir))

from rag_retriever import retrieve_relevant_docs

project_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ai_storage = os.path.join(project_root, 'storage', 'ai')
os.makedirs(ai_storage, exist_ok=True)

os.environ['HOME'] = ai_storage
os.environ['USERPROFILE'] = ai_storage
os.environ['KMP_DUPLICATE_LIB_OK'] = 'TRUE'

from langchain_openai import ChatOpenAI
from langchain_core.messages import HumanMessage, AIMessage, ToolMessage, SystemMessage
from langchain_core.prompts import ChatPromptTemplate, MessagesPlaceholder


def format_few_shot_examples(examples):
    if not examples:
        return 'Tidak ada contoh jawaban tersimpan.'

    blocks = []
    for idx, example in enumerate(examples, start=1):
        query = example.get('query', '').strip()
        response = example.get('response', '').strip()
        if not query or not response:
            continue
        blocks.append(
            f"Contoh {idx}:\nPertanyaan: {query}\nJawaban: {response}"
        )

    return '\n\n'.join(blocks) if blocks else 'Tidak ada contoh jawaban tersimpan.'


def emit_event(event):
    print(json.dumps(event, ensure_ascii=False), flush=True)


def format_ai_error(exc: Exception, model: str) -> dict:
    raw = str(exc)
    payload = {
        'success': False,
        'error': raw,
        'message': raw,
        'model': model,
    }

    try:
        parsed = json.loads(raw)
        if isinstance(parsed, dict):
            message = parsed.get('message') or parsed.get('error') or raw
            payload['error'] = message
            payload['message'] = message
            if isinstance(parsed.get('model'), str):
                payload['model'] = parsed['model']
    except (json.JSONDecodeError, TypeError):
        if 'blocked' in raw.lower():
            payload['message'] = (
                f'Model "{model}" diblokir gateway AI. '
                'Ganti chat_model ke gc/gemini-2.5-flash di Pengaturan.'
            )
            payload['error'] = payload['message']

    return payload


def extract_message_text(content) -> str:
    if content is None:
        return ''

    if isinstance(content, str):
        return content

    if isinstance(content, list):
        parts = []
        for block in content:
            if isinstance(block, str):
                parts.append(block)
            elif isinstance(block, dict):
                text = block.get('text')
                if isinstance(text, str):
                    parts.append(text)
        return ''.join(parts)

    return str(content)


def normalize_tool_calls(response):
    if hasattr(response, 'additional_kwargs') and response.additional_kwargs.get('tool_calls'):
        return response.additional_kwargs['tool_calls']

    if hasattr(response, 'tool_calls') and response.tool_calls:
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
        return normalized_calls

    return None


def build_messages(input_data, kb, few_shot, formatted_history, user_message, context):
    system_prompt_override = input_data.get('system_prompt')
    if isinstance(system_prompt_override, str) and system_prompt_override.strip():
        return [SystemMessage(content=system_prompt_override), *formatted_history, HumanMessage(content=user_message)]

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

CONTOH JAWABAN TERBAIK (FEW-SHOT):
{few_shot_examples}

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

    return prompt.format_messages(
        input=user_message,
        context=context,
        knowledge_base=kb,
        few_shot_examples=few_shot,
        history=formatted_history,
    )


def run_chat():
    try:
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
        few_shot_examples = input_data.get('few_shot_examples', [])
        knowledge_base = input_data.get('knowledge_base')
        stream_mode = bool(input_data.get('stream'))

        if not api_key:
            raise ValueError("API Key is required")

        kb = knowledge_base if isinstance(knowledge_base, str) and knowledge_base.strip() else retrieve_relevant_docs(user_message)
        few_shot = format_few_shot_examples(few_shot_examples)

        llm = ChatOpenAI(
            api_key=api_key,
            model=model,
            base_url=base_url,
            default_headers=headers if headers else None,
            temperature=0.1,
        )

        if tools_def:
            llm = llm.bind_tools(tools_def)

        formatted_history = []
        for msg in history_raw[-10:]:
            if msg.get('role') == 'user':
                formatted_history.append(HumanMessage(content=msg.get('content')))
            else:
                formatted_history.append(AIMessage(content=msg.get('content')))

        if tool_history:
            for interaction in tool_history:
                ast_msg = interaction.get('assistant', {})
                formatted_history.append(AIMessage(
                    content=ast_msg.get('content', ''),
                    additional_kwargs={'tool_calls': ast_msg.get('tool_calls', [])}
                ))
                for res in interaction.get('results', []):
                    formatted_history.append(ToolMessage(
                        tool_call_id=res['tool_call_id'],
                        content=res['content']
                    ))

        full_prompt = build_messages(input_data, kb, few_shot, formatted_history, user_message, context)

        if stream_mode:
            gathered = None
            for chunk in llm.stream(full_prompt):
                gathered = chunk if gathered is None else gathered + chunk
                text = extract_message_text(chunk.content)
                if text:
                    emit_event({'type': 'token', 'content': text})

            tool_calls = normalize_tool_calls(gathered) if gathered is not None else None
            usage = {}
            if gathered is not None and hasattr(gathered, 'response_metadata'):
                usage = gathered.response_metadata.get('token_usage', {}) or {}

            final_text = extract_message_text(gathered.content if gathered is not None else '')

            if tool_calls:
                emit_event({
                    'type': 'tool_calls',
                    'success': True,
                    'content': final_text,
                    'tool_calls': tool_calls,
                    'model': model,
                    'usage': usage,
                })
                return

            emit_event({
                'type': 'done',
                'success': True,
                'content': final_text,
                'model': model,
                'usage': usage,
            })
            return

        response = llm.invoke(full_prompt)
        output = {
            "success": True,
            "content": extract_message_text(response.content),
            "model": model,
            "usage": response.response_metadata.get('token_usage', {})
        }

        tool_calls = normalize_tool_calls(response)
        if tool_calls:
            output['tool_calls'] = tool_calls

        print(json.dumps(output, ensure_ascii=False))

    except Exception as e:
        payload = format_ai_error(e, locals().get('model', 'unknown'))
        if locals().get('stream_mode'):
            emit_event({'type': 'error', **payload})
        else:
            print(json.dumps(payload, ensure_ascii=False))


if __name__ == "__main__":
    run_chat()