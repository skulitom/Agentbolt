# Phone replies

Phone replies are off until configured in **Desk → Settings → Phone replies**. They use your chosen
HTTPS ntfy server, two private topics and access tokens. Nothing creates an account, subscribes your
phone or opens a network listener. The existing count-only push is a separate setting.

Configure the server's access controls before enabling:

- A questions topic that only your Desk account and phone subscriber can read. The Desk publishes here.
- A different replies topic that the Desk can read. Deny anonymous reading **and publishing** on both topics.
- A Desk account token with read/write permission on both topics.
- A separate phone publisher account/token with write permission only on the replies topic. This token
  travels inside action buttons; do not use an administrator token for it.

Enter the HTTPS origin (no path), both topic names and tokens in Settings. Subscribe to the questions
topic in the ntfy app using an account allowed to read it. Blank password fields preserve saved tokens;
**Disable and forget credentials** removes them. Changing the server requires both credentials again.
The Desk does not return saved tokens through its status response. Credentials live in the local
user's database; this is the same-user trust boundary described in the README, not protection from
other programs already running as that user. Keep database backups private.

Before sending, and once a minute while enabled, Agentbolt checks both topics' ntfy authentication
endpoints: anonymous reads must be refused and its configured account must be accepted. It never
publishes an anonymous probe. Publishing ACLs remain the server administrator's responsibility.
All requests use HTTPS, refuse redirects, have deadlines and bounded response sizes. Errors shown
in the Desk contain no upstream response bodies, credentials or answer tokens.

Questions include the owner, full question, suggested answer, options, evidence, deadline and stable
ask ID when this fits the phone message. Reversible choose/approve questions with at most three short
options receive buttons. Irreversible decisions, long questions/options, text and forms direct you to
the Desk. No truncated question has actionable buttons. This preserves the existing restriction on
notification tokens: they are same-user credentials, not a stronger identity check for irreversible work.

Questions respect owner mute, notification pause, priority and the ten-minute owner interval. P2 waits
until due within two hours. At most one new question is published per minute globally. Messages request
ntfy's low priority; keep the app's corresponding notification channel silent. The phone OS and any
custom channel settings control the final sound behavior. [ntfy publishing reference](https://docs.ntfy.sh/publish/).

A button sends a single-use token to the protected reply topic. The daemon verifies its MAC, saves the
receipt and polling checkpoint in one transaction, then starts the installed Desk with only the receipt
ID. The Desk consumes it through its verified pipe. Neither the MCP pipe nor the network worker can
write a human answer. Phone and Windows-toast tokens have different purposes and cannot substitute
for one another. Another answer, cancellation, disabling/reconfiguring phone replies or seven days'
expiry invalidates an outstanding token. Replays cannot change an answer.

The tap confirms only that ntfy accepted the reply. Wait for **Answer recorded** on the questions topic
to know the Desk committed it. A closed/expired question gets a rejection acknowledgement. Receipts
already saved locally survive a restart; a failed Desk start is retried. Unknown/invalid tokens are
ignored. Publication may repeat after an uncertain network failure; repeated buttons remain single-use.
Sequence IDs let supporting ntfy versions replace retry notifications.

Remote messages still depend on the ntfy server's retention period. If the PC stays offline beyond that
period, a phone reply can expire before Agentbolt reads it; the question remains answerable in the Desk.
Oversized/truncated reply history stops advancement and shows an error rather than silently skipping it.
[ntfy subscription API](https://docs.ntfy.sh/subscribe/api/). Refresh phone status in Settings to see the
last protection check, publication, reply check and pending receipts.

This feature is implemented in the candidate. No real account or phone was configured during development;
live phone/service checks belong after the remaining feature work.
