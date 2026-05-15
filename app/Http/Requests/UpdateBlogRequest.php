<?php

namespace App\Http\Requests;

use Illuminate\Foundation\Http\FormRequest;

class UpdateBlogRequest extends FormRequest
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
            'title' => 'sometimes|required|string|max:255',
            'slug' => 'sometimes|required|string|unique:tbl_blog,slug,' . $this->route('blog')->id,
            'content' => 'sometimes|required|string',
            'category' => 'nullable|string|max:255',
            'cover_image' => 'nullable|string|max:255',
            'is_published' => 'nullable|boolean',
            'is_internal' => 'nullable|boolean',
        ];
    }
}
