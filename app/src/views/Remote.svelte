<!--
  Other people's machines.

  The job this page does: a friend runs the server, you are the one who knows
  what to change, and their machine is on the same VPN. Add it once, then edit
  its three config files from here instead of over voice chat.

  Nothing about a server's *process* is here — no start, no console. Those go
  through the local supervisor, which has no reach onto another machine. This
  page edits files, and says so.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";

  import Button from "../lib/Button.svelte";
  import Icon from "../lib/Icon.svelte";
  import Modal from "../lib/Modal.svelte";
  import {
    deleteRemote,
    listRemotes,
    readRemoteFile,
    remoteFingerprint,
    remoteServers,
    saveRemote,
    testRemote,
    trustRemote,
    writeRemoteFile,
  } from "../lib/api";
  import {
    errorMessage,
    type RemoteHost,
    type RemoteServer,
    type ServerFile,
  } from "../lib/types";

  const FILES: [ServerFile, string, string][] = [
    ["properties", "server.properties", "連接埠、難度、白名單…"],
    ["jvmArgs", "user_jvm_args.txt", "JVM 參數，含 -Xmx 記憶體上限"],
    ["runScript", "run.bat", "啟動腳本"],
  ];

  const blank = (): RemoteHost => ({
    id: "",
    label: "",
    host: "",
    // The common case is a friend running this same app; ssh is the escape
    // hatch for a machine that does not.
    transport: "app",
    token: "",
    user: "",
    port: 47285,
    keyPath: null,
    dir: "",
  });

  let hosts = $state<RemoteHost[]>([]);
  let error = $state<string | null>(null);

  /** Per host: the last test result, so each row keeps its own answer. */
  let status = $state<Record<string, string>>({});
  let testing = $state<string | null>(null);
  /** Per host, over the app transport: the servers that machine manages. */
  let servers = $state<Record<string, RemoteServer[]>>({});

  let editOpen = $state(false);
  let draft = $state<RemoteHost>(blank());
  let saving = $state(false);

  /** The host key we are asking the user to vouch for, and its fingerprint. */
  let trustFor = $state<RemoteHost | null>(null);
  let trustPrint = $state("");

  let fileOpen = $state(false);
  let fileHost = $state<RemoteHost | null>(null);
  let fileServer = $state<string | null>(null);
  let fileWhich = $state<ServerFile>("properties");
  let fileText = $state("");
  let fileBusy = $state(false);
  /** Shown inside the editor: the page behind it is covered. */
  let fileError = $state<string | null>(null);

  /** The host whose trash button is asking "are you sure". */
  let confirmingRemove = $state<string | null>(null);

  const fileName = $derived(FILES.find(([f]) => f === fileWhich)?.[1] ?? "");

  async function run<T>(action: () => Promise<T>) {
    try {
      await action();
      error = null;
    } catch (e) {
      error = errorMessage(e);
    }
  }

  const refresh = () =>
    run(async () => {
      hosts = await listRemotes();
    });

  function openAdd() {
    draft = blank();
    editOpen = true;
  }

  function openEdit(host: RemoteHost) {
    draft = { ...host };
    editOpen = true;
  }

  async function save() {
    saving = true;
    await run(async () => {
      await saveRemote({ ...draft, label: draft.label.trim() || draft.host });
      await refresh();
      editOpen = false;
    });
    saving = false;
  }

  const remove = (host: RemoteHost) =>
    run(async () => {
      confirmingRemove = null;
      await deleteRemote(host.id);
      await refresh();
    });

  async function pickKey() {
    const picked = await open({ title: "選擇私鑰檔", multiple: false });
    if (typeof picked === "string") draft.keyPath = picked;
  }

  /**
   * Test, and turn the one failure that has a next step into that step.
   *
   * "Host key verification failed" means ssh has never seen this machine. That
   * is not an error to report, it is a decision to put in front of the user.
   */
  async function test(host: RemoteHost) {
    testing = host.id;
    try {
      status[host.id] = await testRemote(host.id);
      if (host.transport === "app") servers[host.id] = await remoteServers(host.id);
      error = null;
    } catch (e) {
      const message = errorMessage(e);
      if (/host key verification|known_hosts/i.test(message)) {
        await askToTrust(host);
      } else {
        status[host.id] = message;
      }
    } finally {
      testing = null;
    }
  }

  async function askToTrust(host: RemoteHost) {
    trustFor = host;
    trustPrint = "讀取中…";
    try {
      trustPrint = await remoteFingerprint(host.id);
    } catch (e) {
      trustPrint = errorMessage(e);
    }
  }

  async function confirmTrust() {
    const host = trustFor;
    if (!host) return;
    trustFor = null;
    await run(() => trustRemote(host.id));
    await test(host);
  }

  async function openFile(
    host: RemoteHost,
    server: string | null,
    which: ServerFile,
  ) {
    fileHost = host;
    fileServer = server;
    fileWhich = which;
    fileText = "";
    fileError = null;
    fileBusy = true;
    fileOpen = true;
    try {
      fileText = await readRemoteFile(host.id, server, which);
    } catch (e) {
      fileError = errorMessage(e);
    }
    fileBusy = false;
  }

  async function saveFile() {
    if (!fileHost) return;
    fileBusy = true;
    try {
      await writeRemoteFile(fileHost.id, fileServer, fileWhich, fileText);
      fileOpen = false;
      fileError = null;
    } catch (e) {
      fileError = errorMessage(e);
    } finally {
      fileBusy = false;
    }
  }

  onMount(refresh);
</script>

<section class="page">
  <header>
    <div>
      <h1>遠端</h1>
      <p class="sub">編輯別台電腦上的伺服器設定檔。</p>
    </div>
    <Button variant="primary" onclick={openAdd}>
      <Icon name="plus" size={15} />
      新增主機
    </Button>
  </header>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  {#if hosts.length === 0}
    <div class="empty">
      <p><strong>還沒有遠端主機。</strong></p>
      <p>
        對方同樣開著這個 app 的話，請他到「設定 → 遠端存取」打開開關，把畫面上的
        <strong>配對碼</strong>和他的 VPN 位址給你，就這樣。
      </p>
      <p>
        對方沒有這個 app 的話可以改用 SSH：他要開啟 Windows 的
        <code>OpenSSH Server</code>，並把你的公開金鑰加進
        <code>authorized_keys</code>。系統的 ssh 不支援在這裡輸入密碼。
      </p>
    </div>
  {/if}

  <div class="list">
    {#each hosts as host (host.id)}
      <article class="host">
        <div class="head">
          <h2>{host.label}</h2>
          <span class="tag">{host.transport === "app" ? "app" : "ssh"}</span>
          <code class="addr">
            {host.transport === "ssh" ? `${host.user}@` : ""}{host.host}:{host.port}
          </code>
          <span class="spacer"></span>
          <Button onclick={() => test(host)} disabled={testing === host.id}>
            {testing === host.id ? "連線中…" : "連線"}
          </Button>
          <Button onclick={() => openEdit(host)} aria-label="編輯">
            <Icon name="pencil-line" size={15} />
          </Button>
          {#if confirmingRemove === host.id}
            <Button onclick={() => (confirmingRemove = null)}>取消</Button>
            <Button danger onclick={() => remove(host)}>確定移除</Button>
          {:else}
            <Button danger onclick={() => (confirmingRemove = host.id)} aria-label="移除">
              <Icon name="trash" size={15} />
            </Button>
          {/if}
        </div>

        {#if host.transport === "ssh"}
          <p class="dir"><code>{host.dir}</code></p>
        {/if}

        {#if status[host.id]}
          <p class="status">{status[host.id]}</p>
        {/if}

        {#if host.transport === "ssh"}
          <div class="files">
            {#each FILES as [which, name, note] (which)}
              <button class="file" onclick={() => openFile(host, null, which)}>
                <span>
                  <strong>{name}</strong>
                  <small>{note}</small>
                </span>
                <Icon name="pencil-line" size={15} />
              </button>
            {/each}
          </div>
        {:else if servers[host.id]?.length}
          <!-- Over the app transport the far side names its own servers, so the
               files hang off each of them rather than off one fixed folder. -->
          {#each servers[host.id] as server (server.id)}
            <div class="remote-server">
              <h3>{server.name}</h3>
              <div class="files">
                {#each FILES as [which, name] (which)}
                  <button class="file" onclick={() => openFile(host, server.id, which)}>
                    <span><strong>{name}</strong></span>
                    <Icon name="pencil-line" size={15} />
                  </button>
                {/each}
              </div>
            </div>
          {/each}
        {:else}
          <p class="hint">按「連線」載入對方的伺服器清單。</p>
        {/if}
      </article>
    {/each}
  </div>
</section>

<Modal bind:open={editOpen} title={draft.id ? "編輯主機" : "新增主機"} width={480}>
  <div class="form">
    <div class="segmented" role="group" aria-label="連線方式">
      <button
        class:active={draft.transport === "app"}
        onclick={() => ((draft.transport = "app"), (draft.port = 47285))}
      >
        app直連
      </button>
      <button
        class:active={draft.transport === "ssh"}
        onclick={() => ((draft.transport = "ssh"), (draft.port = 22))}
      >
        SSH
      </button>
    </div>

    <label class="field">
      <span>名稱</span>
      <input bind:value={draft.label} placeholder="小明的電腦" />
    </label>

    <div class="pair">
      <label class="field">
        <span>位址</span>
        <input bind:value={draft.host} placeholder="25.1.2.3" />
      </label>
      <label class="field narrow">
        <span>連接埠</span>
        <input type="number" bind:value={draft.port} />
      </label>
    </div>

    {#if draft.transport === "app"}
      <label class="field">
        <span>配對碼</span>
        <input
          value={draft.token ?? ""}
          oninput={(e) => (draft.token = e.currentTarget.value.toUpperCase())}
          placeholder="對方「設定 → 遠端存取」顯示的那組"
          class="code"
        />
      </label>
    {:else}
      <label class="field">
        <span>使用者名稱</span>
        <input bind:value={draft.user} placeholder="對方 Windows 的帳號名稱" />
      </label>

      <label class="field">
        <span>伺服器資料夾</span>
        <input bind:value={draft.dir} placeholder="C:/servers/smp" />
      </label>

      <div class="field">
        <span>私鑰（選填）</span>
        <div class="key-row">
          <code class="key">{draft.keyPath ?? "使用 ssh 預設的金鑰"}</code>
          <Button onclick={pickKey}>選擇…</Button>
          {#if draft.keyPath}
            <Button onclick={() => (draft.keyPath = null)} aria-label="清除">
              <Icon name="close" size={15} />
            </Button>
          {/if}
        </div>
      </div>
    {/if}
  </div>

  {#snippet footer()}
    <Button onclick={() => (editOpen = false)} disabled={saving}>取消</Button>
    <Button
      variant="primary"
      onclick={save}
      disabled={saving ||
        !draft.host.trim() ||
        (draft.transport === "app"
          ? !draft.token?.trim()
          : !draft.user.trim() || !draft.dir.trim())}
    >
      儲存
    </Button>
  {/snippet}
</Modal>

<Modal
  open={trustFor !== null}
  title="第一次連線這台主機"
  width={480}
  dismissible={false}
>
  <p class="lead">ssh 沒有見過這台主機。請跟對方確認指紋一致，再決定要不要信任。</p>
  <pre class="print">{trustPrint}</pre>
  <p class="note">
    信任之後這台主機會寫進你的 <code>known_hosts</code>。如果指紋日後改變，連線會被拒絕——那正是中間人攻擊的樣子。
  </p>

  {#snippet footer()}
    <Button onclick={() => (trustFor = null)}>取消</Button>
    <Button variant="primary" onclick={confirmTrust}>指紋正確，信任</Button>
  {/snippet}
</Modal>

<Modal bind:open={fileOpen} title="{fileHost?.label ?? ''} · {fileName}" width={640}>
  <textarea class="editor" bind:value={fileText} spellcheck="false" disabled={fileBusy}
  ></textarea>
  <p class="note">
    存檔會直接覆蓋對方電腦上的檔案。{fileHost?.transport === "ssh"
      ? "伺服器執行中的話，改動要重開才會生效。"
      : "對方的伺服器執行中時不能存，請他先關掉伺服器。"}
  </p>
  {#if fileError}
    <p class="error" role="alert">{fileError}</p>
  {/if}

  {#snippet footer()}
    <Button onclick={() => (fileOpen = false)}>取消</Button>
    <Button variant="primary" onclick={saveFile} disabled={fileBusy || !fileText}>儲存到遠端</Button>
  {/snippet}
</Modal>

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    gap: 18px;
    padding: 22px 28px 24px;
    overflow: auto;
  }

  header {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 20px;
  }

  /* A wordmark, not a heading. On a geometric face the authority comes from
     the drawing — even stroke, circular bowls — and a heavy weight destroys
     exactly that by thickening the monoline into a slab. Weight stays at 500
     and the letters are pulled together instead; that tightening is what makes
     a large geometric line read as set rather than merely enlarged. */
  h1 {
    font-family: var(--font-display);
    font-size: var(--font-hero);
    font-weight: 500;
    letter-spacing: -0.02em;
  }

  .sub {
    margin-top: 6px;
    color: var(--muted);
    font-size: var(--font-small);
  }

  .empty {
    padding: 20px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    color: var(--muted);
    font-size: var(--font-small);
    line-height: 1.8;
  }

  .empty strong {
    color: var(--fg);
  }

  .empty code,
  .note code {
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--wash);
    font-family: var(--font-mono);
    font-size: var(--font-tiny);
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .host {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 16px 18px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  h2 {
    font-size: var(--font-section);
    font-weight: 600;
  }

  .addr,
  .dir code,
  .key {
    font-family: var(--font-mono);
    font-size: var(--font-small);
    color: var(--muted);
  }

  .spacer {
    flex: 1;
  }

  .dir {
    margin-top: -4px;
  }

  .status {
    padding: 8px 12px;
    background: var(--surface-2);
    border-radius: var(--radius-input);
    color: var(--muted);
    font-size: var(--font-small);
    user-select: text;
  }

  .files {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 10px;
  }

  .file {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 9px 12px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-button);
    color: var(--muted);
    text-align: left;
  }

  .file:hover {
    border-color: var(--accent);
    color: var(--fg);
  }

  .file strong {
    display: block;
    color: var(--fg);
    font-family: var(--font-mono);
    font-size: var(--font-small);
    font-weight: 500;
  }

  .file small {
    color: var(--muted);
    font-size: var(--font-tiny);
  }

  .tag {
    padding: 2px 8px;
    border-radius: var(--radius-pill);
    background: var(--wash);
    color: var(--muted);
    font-family: var(--font-mono);
    font-size: var(--font-tiny);
  }

  .remote-server {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-top: 10px;
    border-top: 1px solid var(--border);
  }

  .remote-server h3 {
    font-size: var(--font-body);
    font-weight: 600;
  }

  .hint {
    color: var(--faint);
    font-size: var(--font-small);
  }

  .segmented {
    display: flex;
    gap: 4px;
    padding: 3px;
    background: var(--wash);
    border-radius: var(--radius-button);
  }

  .segmented button {
    flex: 1;
    height: 32px;
    border-radius: 7px;
    color: var(--muted);
    font-size: var(--font-small);
  }

  .segmented button.active {
    background: var(--surface);
    color: var(--fg);
    font-weight: 600;
  }

  .code {
    font-family: var(--font-mono);
    letter-spacing: 0.06em;
  }

  /* ── modals ────────────────────────────────────────── */

  .form {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .pair {
    display: flex;
    gap: 12px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    flex: 1;
  }

  .field.narrow {
    flex: 0 0 110px;
  }

  .field > span {
    color: var(--muted);
    font-size: var(--font-small);
  }

  .field input {
    height: 40px;
    padding: 0 12px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-input);
    color: var(--fg);
    font: inherit;
    user-select: text;
  }

  .field input:focus {
    border-color: var(--accent);
  }

  .key-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .key {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .lead {
    font-size: var(--font-body);
  }

  .print {
    margin-top: 12px;
    padding: 12px 14px;
    background: var(--surface-2);
    border-radius: var(--radius-input);
    font-family: var(--font-mono);
    font-size: var(--font-small);
    line-height: 1.7;
    white-space: pre-wrap;
    word-break: break-all;
    user-select: text;
  }

  .note {
    margin-top: 12px;
    color: var(--muted);
    font-size: var(--font-small);
    line-height: 1.6;
  }

  .editor {
    width: 100%;
    height: 380px;
    padding: 12px 14px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-input);
    color: var(--fg);
    font-family: var(--font-mono);
    font-size: var(--font-small);
    line-height: 1.7;
    resize: none;
    white-space: pre;
    user-select: text;
  }

  .editor:focus {
    border-color: var(--accent);
  }

  .error {
    padding: 10px 14px;
    background: color-mix(in srgb, var(--error) 10%, transparent);
    border-radius: var(--radius-input);
    color: var(--error);
    font-size: var(--font-small);
    user-select: text;
  }
</style>
