import sys
import json
import easyocr
import cv2
import re
import os

def process_ktp(image_path):
    if not os.path.exists(image_path):
        return {"error": "File not found"}

    # Initialize reader (Indonesian language)
    # gpu=False for compatibility in server environments
    reader = easyocr.Reader(['id'], gpu=False)
    
    # Read image
    img = cv2.imread(image_path)
    
    if img is None:
        return {"error": "Could not read image"}

    # OCR
    results = reader.readtext(img)
    
    text_lines = [res[1] for res in results]
    full_text = " ".join(text_lines)
    
    # Extraction logic
    data = {
        "nik": "",
        "nama": "",
        "alamat": "",
        "raw_text": text_lines
    }
    
    # 1. Extract NIK (16 digits)
    # Sometimes OCR misreads 8 as B or 0 as D, etc. 
    # But usually easyocr is good with digits.
    nik_match = re.search(r'\d{16}', full_text)
    if nik_match:
        data["nik"] = nik_match.group(0)
        
    # 2. Extract Nama
    for i, line in enumerate(text_lines):
        clean_line = line.upper()
        if "NAMA" in clean_line:
            # Check current line or next line
            if ":" in line:
                data["nama"] = line.split(":")[-1].strip()
            elif i + 1 < len(text_lines):
                data["nama"] = text_lines[i+1].strip()
            break
            
    # 3. Extract Alamat
    for i, line in enumerate(text_lines):
        clean_line = line.upper()
        if "ALAMAT" in clean_line:
            if ":" in line:
                data["alamat"] = line.split(":")[-1].strip()
            elif i + 1 < len(text_lines):
                data["alamat"] = text_lines[i+1].strip()
            break

    return data

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(json.dumps({"error": "No image path provided"}))
        sys.exit(1)
        
    image_path = sys.argv[1]
    try:
        result = process_ktp(image_path)
        print(json.dumps(result))
    except Exception as e:
        print(json.dumps({"error": str(e)}))
