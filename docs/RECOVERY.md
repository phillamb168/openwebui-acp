# Recovery and retention

Use one absolute, private state directory per configured instance. Its parent must be a trusted operator-controlled directory. The adapter creates its final directory with mode 0700; pre-existing directories must already have suitable ownership and permissions. Keep it on a local filesystem with working file locks, rename and fsync. Network filesystems are not validated.

Without `--state-dir`, restarting loses local mappings and outbox state. Remote chats and Buzz replies remain. With state, ordinary ACP clients must use `session/load` with the saved ACP session ID and CWD. Buzz thread mappings resume automatically when the same configured bot/relay/channel/thread is requested, even when Buzz gives the new process a new ACP session ID.

Recovery is lazy: opening/loading a conversation or receiving another request in its thread triggers reconciliation. Startup does not publish every pending reply in every channel.

| Journal condition | Behavior | Operator response |
| --- | --- | --- |
| Completed | Verify chat identity, message ancestry and history fingerprint; continue the same chat | Do not edit the saved chat independently while using it through ACP |
| Submitted | Poll/read the known chat; recover its completed answer without resubmitting inference | Inspect Open WebUI if the deadline expires |
| Pending signed reply | Resend identical event ID to the same allowed destination | Repair relay access/credentials; request the same thread again |
| Delivered event | Acknowledge duplicate inbound event without another model call or reply | Send a genuinely new request if needed |
| Saving, saved-before-submit, interrupted or conflicting history | Fail closed with an explicit uncertain-state/conflict error | Inspect the remote chat and any tool effects before deciding on a new session/thread |
| Wrong account/model/endpoint/relay/bot, corrupt state or conflicting process lock | Refuse the state directory | Restore matching configuration or use a separate instance; do not overwrite the journal |
| Capacity reached | Refuse additional growth; no automatic eviction | Finish/inspect pending operations and archive the stopped instance before starting a fresh one |

Do not edit `phase`, message IDs, or delivery records to force a retry. A cancelled HTTP request is not proof that a remote tool did nothing. If a turn's outcome is uncertain, inspect the saved Open WebUI chat and the affected external system before issuing a new instruction. This adapter has no tool-specific rollback.

Key rotation to a new key for the same Open WebUI account is supported; switching accounts with the same state is rejected. Removing an author/channel from the allowlist also blocks publication of a pending reply to that destination. Replacing a Buzz identity requires separate state.

To archive/reset deliberately: stop the client/runner, inspect and resolve pending remote work, move the complete state directory to a protected archive, then point the adapter at a new empty path. This loses continuity and duplicate-event memory; ensure the runner does not replay old mentions into a fresh instance. It does not remove saved Open WebUI chats or Buzz messages. Their retention/deletion must be handled in the respective system.

Pending outbox text is plaintext under owner-only permissions. Protect backups as sensitive data, encrypt the host/storage where needed, and avoid putting journals in source control or issue attachments. Delivered entries retain identifiers so old events can be deduplicated.
