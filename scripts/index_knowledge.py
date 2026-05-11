import os
import sys
from dotenv import load_dotenv
load_dotenv() # Load from .env file

from langchain_community.document_loaders import DirectoryLoader, TextLoader
from langchain_text_splitters import RecursiveCharacterTextSplitter
from langchain_openai import OpenAIEmbeddings
from langchain_chroma import Chroma

def index_docs():
    # 1. Configuration
    api_key = os.getenv('OPENROUTER_API_KEY') or os.getenv('OPENAI_API_KEY')
    if not api_key:
        print("Error: OPENROUTER_API_KEY or OPENAI_API_KEY environment variable not set")
        sys.exit(1)

    docs_path = 'docs'
    persist_directory = 'storage/ai/vector_db'
    
    # Create directory if not exists
    os.makedirs(persist_directory, exist_ok=True)

    print(f"Indexing documents from {docs_path}...")

    # 2. Load Documents
    loader = DirectoryLoader(docs_path, glob="**/*.md", loader_cls=TextLoader, loader_kwargs={'encoding': 'utf-8'})
    documents = loader.load()
    
    if not documents:
        print("No documents found to index.")
        return

    # 3. Split Documents
    text_splitter = RecursiveCharacterTextSplitter(
        chunk_size=1000,
        chunk_overlap=200,
        add_start_index=True
    )
    all_splits = text_splitter.split_documents(documents)
    
    print(f"Split into {len(all_splits)} chunks.")

    # 4. Create Vector Store
    embeddings = OpenAIEmbeddings(
        api_key=api_key,
        base_url="https://openrouter.ai/api/v1"
    )
    
    vectorstore = Chroma.from_documents(
        documents=all_splits,
        embedding=embeddings,
        persist_directory=persist_directory
    )
    
    print(f"Successfully indexed to {persist_directory}")

if __name__ == "__main__":
    # If API key is not in ENV, try to get from first arg
    if len(sys.argv) > 1:
        os.environ['OPENAI_API_KEY'] = sys.argv[1]
        
    index_docs()
