<?php
 
 namespace App\Http\Resources;
 
 use Illuminate\Http\Request;
 use Illuminate\Http\Resources\Json\JsonResource;
 
 class MenuPermissionResource extends JsonResource
 {
     public function toArray(Request $request): array
     {
         return [
             'id' => $this->id,
             'menu_key' => $this->menu_key,
             'menu_label' => $this->menu_label,
             'menu_parent' => $this->menu_parent,
             'allowed_roles' => $this->allowed_roles,
             'is_active' => $this->is_active,
             'created_at' => $this->created_at,
             'updated_at' => $this->updated_at,
         ];
     }
 }
