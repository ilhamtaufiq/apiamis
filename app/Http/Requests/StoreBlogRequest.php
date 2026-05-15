<?php

namespace App\Http\Requests;

use Illuminate\Foundation\Http\FormRequest;
use Illuminate\Support\Str;

class StoreBlogRequest extends FormRequest
{
    /**
     * Determine if the user is authorized to make this request.
     */
    public function authorize(): bool
    {
        return true;
    }

    /**
     * Get the validation rules that apply to the request.
     *
     * @return array<string, \Illuminate\Contracts\Validation\ValidationRule|array<mixed>|string>
     */
    public function rules(): array
    {
        return [
            'title' => 'required|string|max:255',
            'slug' => 'nullable|string|unique:tbl_blog,slug',
            'content' => 'required|string',
            'category' => 'nullable|string|max:255',
            'cover_image' => 'nullable|string|max:255',
            'is_published' => 'nullable|boolean',
            'is_internal' => 'nullable|boolean',
        ];
    }

    protected function prepareForValidation()
    {
        if (!$this->slug && $this->title) {
            $this->merge([
                'slug' => Str::slug($this->title) . '-' . Str::random(5),
            ]);
        }
    }
}
