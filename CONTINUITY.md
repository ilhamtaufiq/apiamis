# Continuity Ledger

- Goal: Limit AI chat capabilities to search and analysis only, removing all data recording skills.
- Constraints/Assumptions: 
  - AI should only interact with the database in a read-only manner.
  - Context should be derived from controllers, models, and their relations.
- Key decisions:
  - Removed all tool/function definitions from `ChatController.php`.
  - Removed tool call handling logic.
  - Updated system prompt to define the AI as a "Read-Only" data analysis assistant.
  - Increased OpenRouter API timeout to 120s and externalized model configuration.
- State:
  - Done: Removed input tools, updated prompt, increased timeout, and set Gemini 2.0 Flash Lite as the new default model.
  - Now: Stabilized AI chat functionality against timeouts.
  - Next: Monitor response times and user feedback.
- Open questions (UNCONFIRMED if needed): None.
- Working set (files/ids/commands):
  - c:\laragon\www\apiamis\app\Http\Controllers\ChatController.php
