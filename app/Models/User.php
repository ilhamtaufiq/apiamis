<?php

namespace App\Models;

// use Illuminate\Contracts\Auth\MustVerifyEmail;
use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Foundation\Auth\User as Authenticatable;
use Illuminate\Notifications\Notifiable;
use Laravel\Sanctum\HasApiTokens;
use Spatie\Permission\Traits\HasRoles;
use App\Traits\Auditable;

class User extends Authenticatable
{
    /** @use HasFactory<\Database\Factories\UserFactory> */
    use HasApiTokens, HasFactory, Notifiable, HasRoles, Auditable;

    public const PROTECTED_FROM_DELETION_EMAILS = [
        'ilhamtaufiq@gmail.com',
    ];

    /**
     * The attributes that are mass assignable.
     *
     * @var list<string>
     */
    protected $fillable = [
        'name',
        'email',
        'password',
        'google_id',
        'avatar',
        'gender',
        'nip',
        'jabatan',
    ];

    /**
     * The attributes that should be hidden for serialization.
     *
     * @var list<string>
     */
    protected $hidden = [
        'password',
        'remember_token',
    ];

    /**
     * Get the attributes that should be cast.
     *
     * @return array<string, string>
     */
    protected function casts(): array
    {
        return [
            'email_verified_at' => 'datetime',
            'password' => 'hashed',
        ];
    }

    /**
     * Get pekerjaan assigned to this user (for pengawas lapangan)
     */
    public function assignedPekerjaan(): \Illuminate\Database\Eloquent\Relations\BelongsToMany
    {
        return $this->belongsToMany(Pekerjaan::class, 'user_pekerjaan', 'user_id', 'pekerjaan_id')
            ->withTimestamps();
    }

    /**
     * Default pengawas role for pekerjaan assignments — skipped when user is konsultan-only.
     */
    public function isProtectedFromDeletion(): bool
    {
        $email = strtolower(trim((string) $this->email));

        return in_array($email, self::PROTECTED_FROM_DELETION_EMAILS, true);
    }

    public function grantPengawasRoleIfEligible(): bool
    {
        if ($this->hasRole('pengawas') || $this->hasRole('konsultan_pengawas')) {
            return false;
        }

        $this->assignRole('pengawas');

        return true;
    }
}
