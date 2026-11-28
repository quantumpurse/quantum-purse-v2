# Khie Wallet-Side Protocol Specification (for a rust-libp2p provider)

Khie is CCC's wallet↔dApp link: a dApp page asks a wallet app to connect and sign, over libp2p through a public relay. CCC defines it only in its TypeScript source, so this document records the wallet side as read from that source, for Quantum Purse v2's Rust implementation. It is a snapshot of the commits below; re-check them when CCC releases a new connector.

Sources and citation keys:

- `ccc:` is ckb-devrel/ccc master at commit `9ffef01` (2026-09-26). All paths are under `packages/`.
- `km:` is Hanssen0/khie-mobile `dev` at commit `48b48aa` (2026-11-10), the reference wallet ("An AI generated simple CKB wallet and Khie Provider").
- `npm:` is a published tarball: `@ckb-ccc/core@1.21.0`, `@ckb-ccc/libp2p@0.2.1`/`0.3.0`, `libp2p@3.3.11`, `@libp2p/webrtc@6.0.32`, `@libp2p/circuit-relay-v2@4.2.13`, `@libp2p/identify@4.1.14`, `@libp2p/utils@7.4.x`.

Labels: **[UNCONFIRMED]** marks anything not proven from source. **[REC]** marks my recommendation for Quantum Purse; it is not part of the protocol.

## 0. The shape in one paragraph

Khie is a pairing protocol plus JSON-RPC carried over libp2p, with a public relay in between. The dApp side ("connector") always ends up as the JSON-RPC **client**. The wallet side ("provider") always ends up as the JSON-RPC **server**. This holds no matter who scanned whose QR code. Both sides run the same pairing service. A pairing is authorised by a one-time 128-bit secret that travels in the QR link. After pairing, the connector calls `get_info`, then `connect` (which the user approves), then the signing methods. Each call uses its own libp2p stream. The protocol survives lost responses through a per-request `request_id` and a `get_result` recovery call. The two sides first talk through a circuit-relay-v2 hop. Browsers then try to move to a direct WebRTC connection. A peer that cannot do WebRTC stays on the relay, which still works.

## 1. libp2p stack

**Connector (browser)**, from `ccc: connector/src/scenes/khie/node.ts:41-64`:

- Listens on `["/p2p-circuit", "/webrtc"]`.
- Transports: `webSockets()`, `webRTC()` (browser build only; `webrtc.ts:3-5` throws outside browsers, and `package.json` "browser" swaps in `webrtc.browser.ts`), and `circuitRelayTransport()`.
- Connection encryption: `noise()`. Stream muxer: `yamux()`.
- Services: `identify()`, `identifyPush()`, and `pairing` (Khie). The connector does **not** register JSON-RPC; it only dials it.
- `peerStore.maxAddressAge: Infinity`, so learned addresses are kept for the whole session.

**Reference providers** use an identical stack and add a `jsonRpc` service:

- CCC app: `ccc: app/src/app/modules/khie/khie-signer-session.ts:402-431`.
- khie-mobile: `km: src/khie/KhieProviderSession.ts:536-592`.

**Protocol IDs** (`node.ts:5,10`; `km: src/khie/protocol.ts:1-2`):

- Pairing: `/nervos-ckb/khie/pairing/0.0.1`
- JSON-RPC: `/nervos-ckb/khie/json-rpc/0.0.1`
- Standard protocols: identify `/ipfs/id/1.0.0`, identify push `/ipfs/id/push/1.0.0` (`npm: @libp2p/identify consts.ts:6-9`), HOP `/libp2p/circuit/relay/0.2.0/hop`, STOP `/libp2p/circuit/relay/0.2.0/stop` (`npm: circuit-relay-v2 constants.ts`), and WebRTC signalling `/webrtc-signaling/0.0.1` (`npm: @libp2p/webrtc constants.ts:120`).

**Peer identity.** Neither side passes a `privateKey`, so js-libp2p generates a fresh **Ed25519** key each time a node is created (`npm: libp2p src/index.ts:202`). Peer IDs therefore look like `12D3KooW…`, and they are ephemeral. Nothing is persisted. khie-mobile documents this: "Pairings are process-scoped. Restarting the app requires pairing again." (`km: README.md:101`).

**Relay.** The default relay is `/dns4/relay.ckbccc.com/tcp/443/wss`:

- Connector: `node.ts:6`.
- CCC app: `khie-signer-session.ts:56`.
- khie-mobile: `km: protocol.ts:3`.

It is configurable through the `khie-relay-address` attribute / `khieRelayAddress` (`ccc: connector/src/connector/index.ts:47-49`, added by PR #525). It is dialled **without** a `/p2p/` suffix, so the relay's peer ID is learned from the Noise handshake. `RelayConnectionController` keeps the connection up:

- Dial timeout: 10 s.
- Retry delays: 5 s, 10 s, 20 s, then every 30 s.
- After a success it sticks to the address that worked.

Source: `ccc: libp2p/src/relayConnection.ts:5-7,77-109`.

Listening on bare `/p2p-circuit` triggers relay discovery. Once identify shows the relay supports HOP, js-libp2p reserves a slot (`npm: circuit-relay-v2 transport/listener.ts:95-117`). The node's addresses then become `<relay>/p2p/<relayId>/p2p-circuit/p2p/<self>`, plus `<relay>/p2p/<relayId>/p2p-circuit/webrtc/p2p/<self>` (fixture: `ccc: libp2p/src/pairingEndpoint.test.ts:12-19`).

**Exact js versions.**

- Connector `package.json` ranges: `libp2p ^3.3.9`, `@libp2p/webrtc ^6.0.30`, `@libp2p/websockets ^10.1.20`, `@libp2p/circuit-relay-v2 ^4.2.12`, `@libp2p/identify ^4.1.13`, `@chainsafe/libp2p-noise ^17.0.0`, `@chainsafe/libp2p-yamux ^8.0.1`, `@libp2p/interface ^3.3.0`, `@multiformats/multiaddr ^13.0.3`.
- Resolved in `ccc: pnpm-lock.yaml`: libp2p 3.3.11, webrtc 6.0.32, websockets 10.1.21, circuit-relay-v2 4.2.13, identify 4.1.14, noise 17.0.0, yamux 8.0.1.
- `@ckb-ccc/libp2p` 0.3.2 depends on `@libp2p/interface ^3.3.0`, `@libp2p/interface-internal ^3.1.12`, `@libp2p/utils ^7.4.0`, `it-length-prefixed ^11.0.1` (`ccc: libp2p/package.json`).
- khie-mobile pins: libp2p 3.5.1, webrtc 6.1.0, websockets 10.1.25, circuit-relay-v2 4.2.17, identify 4.1.18, noise 17.0.0, yamux 8.0.1, `@ckb-ccc/core` 1.23.0, `@ckb-ccc/libp2p` 0.3.2 (`km: package.json`).
- Published latest: core 1.23.0, libp2p 0.3.2, connector 2.3.2, connector-react 2.2.2.

**Limited connections.** Every Khie handler and dial passes `{ runOnLimitedConnection: true }`:

- `ccc: libp2p/src/jsonRpcService.ts:68-72`
- `pairingService.ts:94-98,303-315`
- `jsonRpcTransport.ts:90-101`

So all of Khie works over a relayed (limited) connection. rust-libp2p does not gate protocols this way, so this needs no action beyond serving both protocols on relayed connections.

## 2. Roles, the two QR directions, and the link format

**Roles.** The endpoint `role` is `"provider"` for a wallet and `"connector"` for a dApp:

- Connector encodes its own endpoint with role `connector`: `ccc: connector/src/scenes/khie/session.ts:187-198`.
- Connector only accepts scanned endpoints with role `provider`: `session.ts:277`.
- Providers encode role `provider` and accept role `connector`: `khie-signer-session.ts:138,335-340`; `km: KhieProviderSession.ts:234,434-439`.
- A mismatch throws `PairingEndpointRoleError` (`ccc: libp2p/src/pairingEndpoint.ts:17-27,85-90`). The connector shows it as "This is not a compatible Khie pairing code" (`session.ts:433-437`, `pairing.ts:339-358`).

**Direction A: the wallet scans the connector QR.**

1. The connector displays `https://app.ckbccc.com/#khie?role=connector&addresses=…&secret=…` (`node.ts:7`, `session.ts:187-192`).
2. On the connector, an "Open Wallet" link carries the same endpoint on `khie-wallet://app/connect` (`node.ts:8`, `session.ts:193-198`, `pairing.ts:207-216`; PR #550).
3. The wallet dials the addresses through the connector's relay. khie-mobile first does `node.dial(addresses)` with a 10 s timeout, retrying every 2 s (`km: KhieProviderSession.ts:238-280`).
4. The wallet then runs the pairing **client** (`PairingService.pair`).
5. The connector's pairing **server** accepts, emits `onPaired`, and its `acceptPeer` opens JSON-RPC to the wallet (`session.ts:221-223,290-328`).

**Direction B: the connector scans the wallet QR (or pastes the link).**

1. The wallet's QR is `<endpointUrl>#?role=provider&addresses=…&secret=…`. The base URL is free-form, because only the fragment is parsed:
   - CCC app uses `https://app.ckbccc.com/#khie` (`khie-client-module.tsx:63`).
   - khie-mobile uses `https://app.ckbccc.com/khie` (`km: App.tsx:163`).
2. The connector runs the pairing **client** against those addresses (`session.ts:277-278`).
3. On success it emits `onPaired`, then `acceptPeer`, then JSON-RPC.

**Endpoint encoding** (`ccc: libp2p/src/pairingEndpoint.ts:29-70,116-120`):

1. Encode each multiaddr in its binary form, prefixed by an unsigned-varint length, and concatenate (`it-length-prefixed`).
2. Limits: 1–16 addresses, and at most 16384 bytes before compression (`pairingEndpoint.ts:6-7,35-47`).
3. Compress with `CompressionStream("deflate")`. That is **zlib format (RFC 1950)**: header `78 9c`, with an Adler-32 trailer. I verified it locally; khie-mobile uses fflate `Zlib` (`km: pairingEndpointCodec.ts:129-137`).
4. Encode with **base64url, no padding** (`uint8array-extras` strips `=`; `km: pairingEndpointCodec.ts:168-170`).
5. Write parameters into the **URL fragment**, after the existing fragment "route" and a `?`, as `application/x-www-form-urlencoded`. Order: `role` (only if set), then `addresses`, then `secret`. Any existing `addresses`/`role`/`secret` in the fragment are deleted first (`pairingEndpoint.ts:56-67`).
6. Parameters in the real query string are ignored (test `pairingEndpoint.test.ts:114-122`).

**Endpoint decoding** (`pairingEndpoint.ts:72-150`):

- Trims the input.
- Reads `role`, `addresses`, and `secret` from the fragment.
- Inflates with a 16384-byte output cap.
- Rejects more than 16 addresses, or zero addresses.

Each address **must end in `/p2p/<peerId>`**. The js circuit dial needs the destination peer ID, and the peer ID is not carried anywhere else. Examples generated locally with the test addresses:

```
https://app.ckbccc.com/#khie?role=connector&addresses=eJzLNRMoSs1JrNRLzk5KTk7WS87PZWPcfY95KbMagwoHo5DCgxU_C5c8TjCzcLS2Y_F3WTeFX0XOYWZtSuB9tw2TbhXKLmKayQRX3Ph4bX3sjCO1Jxe2PK-41rjh1OKmkCfVz67pzG4MbCpldsim3Dai7QIAyVRYBA&secret=AAECAwQFBgcICQoLDA0ODw
khie-wallet://app/connect#?role=connector&addresses=<same>&secret=<same>
https://app.ckbccc.com/khie#?role=provider&addresses=<same>&secret=<same>
```

The decoded `addresses` value is `[uvarint 109][109 bytes of /dns4/relay.ckbccc.com/tcp/443/wss/p2p/<relay>/p2p-circuit/webrtc/p2p/<peer>][uvarint 107][…/p2p-circuit/p2p/<peer>]`.

**Deep link.** The format is `khie-wallet://app/connect#?role=connector&addresses=…&secret=…`. khie-mobile requires all of the following (`km: src/khie/connectDeepLink.ts:10-21`):

- scheme `khie-wallet:`
- host `app`
- path `/connect`
- no userinfo, port, or query
- a fragment that starts with `#?`
- a fragment that decodes with expected role `connector`

The Android intent filter is scheme `khie-wallet`, host `app`, path `/connect` (`km: app.json:12-23`). [REC] On desktop, register the `khie-wallet` URL scheme with the operating system.

**Secret.** The secret is 16 bytes from `crypto.getRandomValues`, encoded as base64url without padding, so it is 22 characters (`pairingService.ts:438-440`). There is **no** short verification code (SAS). The only authenticators are (a) the secret and (b) the Noise-authenticated peer ID inside the QR addresses.

## 3. Pairing protocol `/nervos-ckb/khie/pairing/0.0.1`

**Framing** (`ccc: libp2p/src/pairingService.ts:343-362,389-396`, `@libp2p/utils stream-utils.ts:312-404`). The protocol uses one stream per exchange:

1. The client writes **uvarint(len) ‖ UTF-8 JSON**.
2. The client half-closes its write side (yamux FIN; `stream.close()` means "close for writing", per `@libp2p/interface stream.ts:48-59`).
3. The server reads exactly one frame, writes one frame, and calls `close()`.
4. The client reads one frame, then `closeRead()`.

Limits: `MAX_PAIRING_MESSAGE_LENGTH = 1024` bytes of payload (`pairingService.ts:12`). The length prefix may be at most `encodingLength(1024) = 2` bytes, so Rust must use minimal varints (`stream-utils.ts:315-319`).

**Messages** (`pairingService.ts:36-40`):

```json
→ {"type":"pair","secret":"<22-char base64url>","name":"<optional self label>"}
→ {"type":"unpair"}
← {"ok":true,"name":"<optional responder label>"}
← {"ok":false,"error":"Pairing rejected"}
```

**Request parsing:**

- `type` must be `"pair"` with a non-empty string `secret`, or `"unpair"`.
- A non-string `name` is dropped.
- Unknown fields are ignored.
- Anything else throws, and the stream is aborted with no response (`pairingService.ts:398-419,234-239`).

**Request building.** The client drops `name` if the JSON exceeds 1024 bytes, and errors if it still does not fit (`pairingService.ts:364-387`).

Labels in use:

- The connector's label is the dApp `appName` (`session.ts:146-150`, `pairing.ts:56-57`).
- khie-mobile sends `"Khie Wallet"` (`km: KhieProviderSession.ts:545`).
- The CCC app sends none.
- The connector ignores the wallet's pairing label (`session.ts:221-223`). The display name comes from `get_info`.

**Server side** (`pairingService.ts:192-240`):

- **`unpair`:** remove the peer (notifying listeners) and always answer `{"ok":true}`.
- **`pair`:** reject with `{"ok":false,"error":"Pairing rejected"}` if either of these holds:
  - `secret !== currentSecret`. This is a plain JS comparison. [REC] Use a constant-time comparison in Rust.
  - `canPair(remotePeer)` is false.
- **On accept:** these steps run in this order:
  1. **Rotate the secret immediately**, before any await. This consumes the one-time credential, so of two concurrent requests only one wins (test `pairingService.test.ts:124-157`).
  2. Mark the peer paired.
  3. Write `{"ok":true,"name":…}`.
  4. Notify `onPaired`.

**Client side** (`pairingService.ts:123-165`):

1. Open the stream to the target addresses and learn the remote peer from the connection.
2. Mark the remote paired **before** the exchange.
3. Send `pair`.
4. On `ok:true`, rotate the client's **own** secret too and notify `onPaired`.
5. On failure, un-mark the peer.

**Single-peer authorisation.** `canPair` is false once a peer is selected or paired:

- Connector: `!signal.aborted && !resources.selectedPeer` (`session.ts:147`).
- CCC provider: `!aborted && !pairedPeer` (`khie-signer-session.ts:183`).
- khie-mobile: the same, plus `pairingEnabled`, which is only true while the user is pairing (`km: KhieProviderSession.ts:124-128,224,311-316`).

The provider's JSON-RPC guard rejects every peer except the paired, selected one, with `-32000 "Peer is not paired for Khie access"` (`khie-signer-session.ts:417-428`; `km: KhieProviderSession.ts:559-569`).

**Liveness.** `pairedPeerTimeoutMs = 30 min` (`node.ts:11`, `khie-signer-session.ts:60`):

- A paired peer is dropped (and `onUnpaired` fires) after 30 min with **no open connection**.
- While any connection is open, the timer re-arms (`pairingService.ts:280-296`).
- `refresh(peer)` restarts the timer. It is called on every JSON-RPC response on the connector (`session.ts:305`), on every JSON-RPC request on the provider (`khie-signer-session.ts:428`), and on `peer:connect`/`peer:disconnect` (`khie-signer-session.ts:211-222`).

**Unpair.** `unpair(peer)` removes the peer locally, then sends `{"type":"unpair"}` with a 10 s timeout (`pairingService.ts:13,167-190`). The connector sends `unpair` in three cases:

- `get_info` fails (`session.ts:316-322`).
- The dApp disconnects or the signer is replaced (`session.ts:337-341,389-418`).
- It closes.

When the connector receives `unpair`, it calls `signer.replace()` and closes (`session.ts:343-356`). Providers start a fresh JSON-RPC provider session on unpair (`khie-client-module.tsx:614-629`; `km: App.tsx:1267-1272`).

**Address exchange.** `@ckb-ccc/libp2p` ≤0.2.1 sent `addresses?: string[]` inside `pair` and opened a "fallback connection" to them (`npm: libp2p-0.2.1 pairingService.ts:42,226-244,269-283,397-410`). This was **removed in 0.3.0**: the 0.3.0 file is identical to master. Addresses are now learned only through identify and identify-push, and kept with `maxAddressAge: Infinity` (`libp2p CHANGELOG.md:46-48`). A Rust server should ignore an `addresses` field.

**[REC] Ordering rule.** Authorise the peer for JSON-RPC **before** sending your `pair` (Direction A) or your `ok` (Direction B). The connector's first `get_info` follows immediately. A `-32000` reply is a `JsonRpcError`, which the client does not retry (`signer.ts:280-284`), so the connector would unpair (`session.ts:316-322`).

## 4. JSON-RPC protocol `/nervos-ckb/khie/json-rpc/0.0.1`

**Framing.** There is one stream per request. The framing matches pairing, with a 1 MiB limit (`DEFAULT_MAX_MESSAGE_LENGTH = 1048576`; prefix at most 3 bytes):

1. The client writes the request frame and calls `stream.close()`, which is a FIN (`jsonRpcTransport.ts:59-71`).
2. The server reads one frame, runs the handler, writes one response frame, and calls `close()` (`jsonRpcService.ts:101-148`).

Server-side error handling:

- Malformed JSON, or a missing `jsonrpc:"2.0"`, numeric/string `id`, or string `method`, aborts the stream with no response (`jsonRpcService.ts:149-191`).
- A handler exception that is not a `JsonRpcError` becomes `-32603` with the exception's message (`jsonRpcService.ts:169-178`).

Client-side stream selection:

- The client prefers existing open connections: direct ones first, then lowest RTT. It falls back to `dialProtocol(peerId)` (`jsonRpcTransport.ts:83-121`).
- Before each request it fires an opportunistic `dialKnownAddresses` (`dial.ts:4-29`).

**Envelopes.**

- The request is `{"id":<number>,"method":…,"params":[…],"jsonrpc":"2.0"}`. The `id` is a per-requestor counter starting at 0 (`core/src/jsonRpc/requestor.ts:240-247`).
- The response must echo `id` with strict equality (`requestor.ts:223-225`). Any non-null `error` is thrown as `JsonRpcError(code, message, data)` (`requestor.ts:226-228`).
- Success: `{"jsonrpc":"2.0","id":N,"result":<value>}`.
- Error: `{"jsonrpc":"2.0","id":N,"error":{"code":C,"message":"…","data"?:…}}`. `data` is omitted when undefined (`jsonRpcService.ts:112-141`).

**Metadata (core ≥1.22.0).** `params[0]` is always a metadata object, and the method arguments follow (`signer.ts:251-279`):

- `{"request_id":"0x<32 hex>","session_id":"0x<32 hex>"}` is the form for every call except the two below.
- `get_info` omits `session_id`: `{"request_id":"0x…"}`.
- `get_result` sends `{}`.

`request_id` is `hexFrom(randomBytes(16))`, and it is **reused on every retry** of the same call. The JSON-RPC `id` changes on each retry (`signer.ts:257-277`). The server's metadata check is: must be a non-null object, and if present, `request_id` and `session_id` must be strings; otherwise `-32602 "Invalid request metadata"` (`handler.ts:438-455`).

**Server algorithm** (`ccc: core/src/signer/jsonRpc/handler.ts:210-273`):

1. If `params` is not an array, return `-32602 "<m> expects positional parameters"`.
2. Parse the metadata.
3. If the method is `get_result`, require exactly 2 params and a non-empty string at `params[1]`, then go to the recovery logic below. There is **no session check** for `get_result`.
4. Otherwise require a non-empty `request_id` (`-32602 "Invalid request ID"`).
5. If the `request_id` has been seen before, live or in history, return `-32005 "Request ID already exists"`.
6. Record the request **before** running it. The remaining checks run inside the recorded execution (`handler.ts:323-338`):
   - `get_info` must have no `session_id` (`-32602 "get_info does not accept a session ID"`).
   - Every other method needs `session_id == this.id` (`-32004 "Invalid or expired session"`).
   - An unknown method returns `-32601 "Unsupported method: <m>"`.
   - `params.length` must equal the method's argument count + 1 (`-32602 "<m> expects N parameter(s)"`).
   - Every method except `get_info` and `connect` needs a successful `connect` first (`-32001 "Connect must be approved before this request"`).
7. The session ID is `0x` plus 32 hex digits, created once per provider session (`handler.ts:69`). `get_info` is computed once and cached (`handler.ts:94-98`).

**Recovery: `get_result`** (`handler.ts:22-25,323-392`).

Request: `{"method":"get_result","params":[{},"0x<request_id>"]}`. Responses:

- `{"status":"not_found"}`
- `{"status":"pending"}`, returned after the server waits up to **10 s** for completion (`GET_RESULT_WAIT_MS`).
- `{"status":"completed","result":<value>}`. The **`result` key must be present even when the value is null**, because the client checks `"result" in result` (`signer.ts:365`).
- If the original request **failed**, `get_result` re-throws the same error as a JSON-RPC error response.

Retention (`handler.ts:340-354`):

- A completed result stays in the live map for **120 s**.
- It then moves to a history capped at **128** entries.
- Retrieved entries are evicted first.

**Client timing** (`signer.ts:31-36,251-377`; tests `index.test.ts:190-430`):

- Each attempt has a **10 s** transport timeout.
- If a transport error occurs (not a `JsonRpcError`), the client retries the same `request_id` after 5, 10, and 20 s, for 4 attempts in total.
- A `JsonRpcError` ends the call at once, except `-32005`, which (like exhausted retries) moves the client into recovery.
- In recovery, `get_result` uses a 20 s timeout and retries on transport errors after 1, 2, 4, and 8 s, then every 10 s indefinitely.
- On `pending`, the client polls again, but no sooner than 5 s after the previous poll started. With the server's 10 s wait, this is effectively a long poll.
- On `not_found`, the client throws the original transport error.
- `-32004` at any point makes the client call `replace()`, which disconnects the dApp signer (`signer.ts:304-311`).

**Consequence for the wallet.** A user approval may take minutes. The first stream is reset after 10 s, so the wallet must **decouple execution from the stream**:

- Keep running the request and cache its outcome under `request_id`.
- Answer the retry with `-32005`.
- Serve `get_result`.

Both CCC and khie-mobile keep one provider session that outlives the stream; see the comment at `km: App.tsx:1255-1256`. The provider's libp2p service also aborts streams after `DEFAULT_TIMEOUT = 30 s` (`jsonRpcService.ts:7,89-96`), but the session's work continues regardless.

Before core 1.22, `@ckb-ccc/libp2p` ≤0.2.1 had a transport default timeout of 120 s and no recovery (`npm: libp2p-0.2.1 jsonRpcTransport.ts:7`). That is where the "120 s" figure comes from.

**Ordering and concurrency.** There are no ordering guarantees. The client may issue calls in parallel; for example `getAddressObjs` and `getIdentity` are cached independently (`signer.ts:159-218`). Approvals are queued by the provider UI (`khie-client-module.tsx:237-263`).

**Error codes** (`handler.ts:32-41`):

| Code | Name | Meaning or client effect |
|---|---|---|
| -32601 | MethodNotFound | Unknown method. |
| -32602 | InvalidParams | Bad params or metadata. |
| -32000 | ServerError | "No signer is connected", or "Peer is not paired for Khie access". |
| -32001 | InvalidState | Connect is required first. |
| -32002 | NetworkMismatch | "Signer uses X, expected Y". |
| -32003 | UserRejected | "User rejected request". |
| -32004 | InvalidSession | Client disconnects. |
| -32005 | DuplicateRequestId | Client recovers with `get_result`. |
| -32603 | Internal | Generic wrapper for non-`JsonRpcError` exceptions. |

## 5. Methods

Source: `handler.ts:89-196` for the server, `signer.ts` for the client. The client sends calls in this sequence: `SignerJsonRpc.new` → `get_info`; `signer.connect()` → `connect`; then any of the others.

| Method | Params after metadata | Result |
|---|---|---|
| `get_info` | none | `{"session_id":"0x…","type":<SignerType>,"sign_type":<SignerSignType>,"name"?:string,"icon"?:string}` (`transformers.ts:13-19`) |
| `connect` | `"ckb-mainnet"` or `"ckb-testnet"` | `null`, after user approval |
| `get_scripts` | none | `[{"code_hash":"0x…","hash_type":"type"\|"data"\|"data1"\|"data2","args":"0x…"}, …]`. The first entry is the recommended address. |
| `get_native_address` | none | string address |
| `get_identity` | none | string |
| `sign_message` | `{"type":"string","value":"hello"}` or `{"type":"bytes","value":"0x0102"}` | string signature, after approval |
| `prepare_transaction` | Tx JSON | Tx JSON, with no approval |
| `sign_transaction` | Tx JSON | signed Tx JSON, after approval |

Notes on `get_info`:

- `name` defaults to "Khie" in the connector.
- `icon` is used only if it is an `https:` URL or a `data:image/…` URI (`connector/src/scenes/khie/wallet.ts:6-31`).
- `session_id` must be non-empty. An empty one is never sent back (`signer.ts:271`).

Notes on `connect`:

- The network ID comes from the dApp client's address prefix: `ckb` maps to `ckb-mainnet`, `ckt` to `ckb-testnet`. Any other prefix throws in the **connector** (`network.ts:1-15`, `signer.ts:99-100`). Devnets are therefore impossible.
- `connect` first sets `connected = false`.
- The wallet must switch to the requested network or answer `-32002`.

Notes on `get_scripts`:

- The connector rebuilds addresses with its own prefix (`signer.ts:159-184`).
- [REC] Return at least one script.

Notes on `get_native_address` and `get_identity`:

- `get_native_address` is `getInternalAddress()`; for CKB signers it is the recommended full address.
- `get_identity` defaults to the internal address (`signer/signer/index.ts:194-196`). For `SignerCkbPublicKey` it is the 33-byte compressed public key as hex (`signerCkbPublicKey.ts:43-45`).

Notes on `sign_message`:

- The message's `value` is passed on raw (`transformers.ts:21-22,45-66`).
- The result is opaque to the client. For `CkbSecp256k1` it is `0x` plus 65 bytes (r‖s‖v) over `ckbHash("Nervos Message:" + (string or 0x-hex))` (`secp256k1Signing.ts:16-25,75-79`).

**Tx JSON.** This is CKB RPC snake_case with hex quantities, produced by `JsonRpcTransformers.transactionFrom` (`core/src/client/jsonRpc/transformers.ts:84-180`, `types.ts:3-41`):

```json
{"version":"0x0",
 "cell_deps":[{"out_point":{"index":"0x0","tx_hash":"0x…"},"dep_type":"code"}],
 "header_deps":[],
 "inputs":[{"previous_output":{"index":"0x1","tx_hash":"0x…"},"since":"0x0"}],
 "outputs":[{"capacity":"0x174876e800","lock":{"code_hash":"0x…","hash_type":"type","args":"0x…"}}],
 "outputs_data":["0x"],
 "witnesses":["0x"]}
```

Details of the format:

- `dep_type` is `"code"` or `"dep_group"`.
- An output's `type` is **omitted** when absent. On parse, both `null` and absent are accepted (`apply`, `utils/index.ts`).
- Quantities are minimal hex from `numToHex`.
- There is no `hash` field.
- **Inputs carry no cell contents.** The wallet must resolve inputs from its own CKB node or indexer.
- `outputs_data` is padded with `"0x"` up to the number of outputs (`ckb/transaction.ts:1324-1331`).

On `prepare_transaction`: dApps call it during `completeFee` (`ckb/transaction.ts:2505,2554`), possibly many times, so it must be fast and silent. CCC's secp signer adds its cell deps and a placeholder witness of the right size (`signerCkbPublicKey.ts:139-153`). [REC] For Quantum Purse, add the SPHINCS+ lock cell deps and a witness placeholder sized for the SPHINCS+ signature, so fee estimation is correct.

## 6. SignerType and SignerSignType

From `signer/signer/index.ts:23-43`:

- `SignerType`: `EVM`, `BTC`, `CKB`, `Nostr`, `Doge`.
- `SignerSignType`: `Unknown`, `BtcEcdsa`, `EvmPersonal`, `JoyId`, `NostrEvent`, `CkbSecp256k1`, `DogeEcdsa`.

The client copies both values without validating them (`transformers.ts:35-43`).

How the connector uses them:

- `type` only selects the signer icon (`connector/src/scenes/connected.ts:141`, `selecting/signers.ts:14-18`).
- `sign_type` is placed into the `Signature` that `signMessage` returns (`index.ts:384-390`).
- `Signer.verifyMessage` dispatches on `sign_type`, and **throws "Unknown signer sign type"** for `Unknown` (`index.ts:106-150`).

khie-mobile uses `SignerCkbPublicKey`, so it reports `CKB`/`CkbSecp256k1` (`km: khieSignerAdapter.ts:24-30`).

[REC] Quantum Purse should report `type:"CKB"` and `sign_type:"Unknown"`. Any secp256k1 value would make dApps verify a SPHINCS+ signature with the wrong algorithm. With `Unknown`, CCC-side `verifyMessage` throws, so dApp logins that verify signatures will not work until CCC adds a SPHINCS+ sign type.

## 7. WebRTC upgrade and relay fallback

**How the browsers upgrade.** Browser nodes listen on `/webrtc`, which advertises `<circuit>/webrtc` addresses. The upgrade path is `@libp2p/webrtc` **private-to-private**:

1. An SDP offer, the answer, and ICE candidates travel over the relayed connection on `/webrtc-signaling/0.0.1`, as varint-framed protobuf `Message{type: SDP_OFFER|SDP_ANSWER|ICE_CANDIDATE, data}` (`npm: webrtc private-to-private/pb/message.proto`).
2. The resulting connection is upgraded with `skipEncryption: true` and `skipProtection: true`, so there is no Noise. Streams are multiplexed over RTCDataChannels (`private-to-private/transport.ts:197-201`).
3. Default STUN servers are Google, Twilio, Cloudflare, and Mozilla (`constants.ts:13-16`).

The result has `connection.direct === true`.

**What drives the upgrade on the connector:**

- `KhieConnectionController` requires a **direct** connection for the first 4 attempts. It dials known addresses at 0, 5, 10, and 20 s, then accepts a relayed connection (`connector/src/scenes/khie/connection.ts:6-9,88-133`; test `connection.test.ts:191-209`).
- It restarts on `peer:disconnect` and on `visibilitychange`.
- `dialKnownAddresses` appends `/p2p/<peer>` to peer-store addresses so libp2p can upgrade without `force` (`dial.ts:10-29`).
- js-libp2p never reuses a *limited* connection for a new dial. It reuses an unlimited indirect one only if no candidate address is direct (`npm: libp2p connection-manager/utils.ts:104-151`).

**A peer without WebRTC** (such as rust-libp2p native) never advertises `/webrtc`:

- The connector keeps using the relayed connection.
- It may open further relayed connections after each dial.
- Every request still works, because all Khie protocols run on limited connections.

As of 2026-11, rust-libp2p has **no merged browser-to-browser `/webrtc` transport**. PR #5978 is open and targets `webrtc-websys` (wasm) only; there is no native implementation ([PR #5978](https://github.com/libp2p/rust-libp2p/pull/5978), [libp2p WebRTC docs](https://libp2p.io/docs/webrtc/)).

When the wallet dials a connector endpoint, it must skip `/webrtc` addresses and dial the plain `/p2p-circuit/p2p/<peer>` one.

**Relay limits, measured on 2026-11-10.** `relay.ckbccc.com` runs `js-libp2p/3.3.8 node/24.19.0`, peer id `12D3KooWNus7APspZcz3mAhaJYqhyGSkZxGBjzhfqN9NC7j4Q66f`, and offers only identify and `/libp2p/circuit/relay/0.2.0/hop`. Its reservation reply announced `limit: { duration: 600000, data: 67108864 }` with a reservation `expire` 1800 s ahead, granted to a freshly generated peer id with no allowlist:

- **Data:** 64 MiB per relayed connection. The js relay server keeps one budget for both directions together and counts encrypted bytes. A 2 MiB transfer passed untouched.
- **Duration:** 10 minutes per relayed connection. The js server stores `duration` in milliseconds (`AbortSignal.timeout(limit.duration)`), while the circuit-v2 spec and the js and Rust clients read it as seconds, so a client shows about 6.9 days. Do not schedule from the client's value. An idle relayed connection was still open at 185 s.
- **Reservation:** 30 minutes, renewed by the client before expiry.

The largest SPHINCS+ bind signature (SHA2/SHAKE-256f, 49,925 bytes in the witness layout, about 100 KB as hex) uses a tenth of the 1 MiB message cap and a small fraction of the connection budget. A request still open when the 10-minute cut lands fails and is recovered through `get_result`.

## 8. Versions, compatibility, and the minimum provider

**The breaking change was core 1.22.0, not 1.23.** PR #547 "Add provider sessions and recover lost signer JSON-RPC responses" (`ccc: core/CHANGELOG.md:90-91`). Compare:

- `npm: core-1.21.0 handler.ts:27-205`: plain params with an exact argument count, no metadata, no session, no `get_result`, and `get_info` without `session_id`.
- `npm: core-1.22.0`: the `signer/jsonRpc/*` sources are byte-identical to master.

Version mapping:

- Connector 2.2.3 and later (libp2p 0.3.0, ccc 1.3.4, core 1.22.0; `connector/CHANGELOG.md:41-49`) speak the metadata protocol.
- Connector 2.0.0–2.2.2 speak the old one.
- khie-mobile (core 1.23.0) therefore works only with connector 2.2.3 and later.

[REC] For optional backward compatibility, detect the protocol by whether `params[0]` is an object with a string `request_id`. A bare `typeof object` check is not enough, because old `sign_message` and `prepare_transaction` parameters are also objects.

**Minimum provider for the current connector:**

1. **libp2p.** An Ed25519 identity, `/dns4/…/tcp/443/wss` (TLS WebSocket plus DNS), Noise XX, yamux, and identify (push recommended).
2. **Circuit-relay-v2 client.**
   - Reserve on the relay and accept STOP.
   - Dial `/p2p/<relay>/p2p-circuit/p2p/<peer>` addresses.
   - Advertise `<relay>/p2p/<relayId>/p2p-circuit` through identify. Without this, the connector cannot re-dial after the relayed connection drops, because its fallback is `dialProtocol(peerId)` using peer-store addresses.
3. **Pairing.** Server and client as in §3:
   - Role checks.
   - A one-time secret that rotates on success, in both roles.
   - Single-peer `canPair`.
   - `unpair` handling.
   - A 30-minute no-connection expiry.
   - The 1024-byte frame limit.
4. **Endpoint codec, QR, and deep link** as in §2. Every address must carry `/p2p/<walletPeerId>`.
5. **JSON-RPC server** as in §4–5:
   - The 1 MiB limit.
   - Requests only from the paired peer.
   - A session ID, the request-ID cache, `get_result` long-poll, and error caching.
   - Exact parameter counts.
   - Explicit `"result":null`.
6. **Not required:** WebRTC. Leaving it out only costs direct-path latency and adds relay churn.

[REC] Implementation notes for rust-libp2p:

- Use raw streams (`libp2p-stream`) rather than `request_response`, whose built-in request timeout and codec assumptions fight the "execute beyond the stream" rule.
- Treat the client's FIN after the request as normal, not as cancellation.
- Expect several simultaneous connections from the same peer.

**Tested on 2026-11-10:** a native rust-libp2p 0.57.1 node (features `tokio, tcp, dns, websocket, noise, yamux, relay, identify, macros, ed25519`, plus `libp2p-stream`) reserved a slot on `relay.ckbccc.com`, was dialled by a js-libp2p peer over the relay and dialled one, and exchanged length-prefixed request/reply on both Khie protocol ids in both directions. The js peer ran in Node with CCC's package versions, without WebRTC. The relay listen address needs the relay's peer id: `/dns4/relay.ckbccc.com/tcp/443/wss/p2p/<relayId>/p2p-circuit`.

**[UNCONFIRMED]:** the real CCC connector in a browser against a provider that never upgrades to WebRTC. Expected to work from code reading, not yet tested.

## 9. What Quantum Purse v2 implements

Decided on 2026-11-11:

- **Relay only.** Native rust-libp2p has no browser-to-browser WebRTC transport, and every Khie protocol already works over the relay.
- **Methods first needed for CKB DAO v2 address binding:** `get_info`, `connect`, `get_scripts`, `get_native_address`, `get_identity`, `sign_message`, and `get_result`. `prepare_transaction` and `sign_transaction` answer `-32601` until they are built: they need the inputs resolved from the wallet's own node and witnesses sized for SPHINCS+.
- **`sign_message` signs like a bind signature.** The message bytes (`type: "string"` as UTF-8, `type: "bytes"` as the decoded bytes) are signed with `KeyVault::ckb_sign`, and the result is `0x` + hex of the quantum-resistant lock's witness layout: header ‖ flag ‖ public key ‖ signature. The public key has to travel with the signature, because an address commits only to a hash of it. The DAO verifier checks exactly this layout.
- **A 32-byte message is refused.** The lock script verifies a spend as a signature, with this same scheme and no prefix, over the transaction's 32-byte `CKB_TX_MESSAGE_ALL`. So a "message" signature over 32 bytes could be a valid spend of the account, handed to the page. A signature over any other length cannot. The DAO's challenges are 64 bytes (the hex digest's ASCII). Domain separation for bind signatures is the lasting fix (DAO v2 BACKLOG #27); this guard holds until then.
- **One approval per batch.** `sign_message` requests the paired page sends together are shown in one dialog and signed under one unlock (`ckb_sign_batch`), instead of one approval each.
- **Identity:** `type: "CKB"`, `sign_type: "Unknown"`. CCC has no SPHINCS+ sign type, and claiming a secp256k1 one would make dApps verify with the wrong algorithm; the cost is that CCC's own `verifyMessage` cannot check Quantum Purse signatures yet.
- **Pairing on desktop:** paste the dApp's pairing link, or open it through the `khie-wallet://app/connect` link scheme, which the app registers with the operating system.
