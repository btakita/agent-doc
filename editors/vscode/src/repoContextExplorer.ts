import * as vscode from 'vscode';
import { execFile } from 'node:child_process';
import { createHash, randomBytes } from 'node:crypto';
import { promises as fs } from 'node:fs';
import * as path from 'node:path';
import {
    currentRepoContextDefinition,
    exchangeQuestionOffset,
    formatRepoContextQuestion,
    parseRepoContextAnchors,
    parseRepoContextCalls,
    parseRepoContextMarkdownFiles,
    pathInRepo,
    planExchangePromptInsertion,
    repoContextAnswer,
    type RepoContextAnchor,
    type RepoContextLink,
    type RepoContextMode,
    type SourceLocation,
} from './repoContextModel.js';

const searchLimit = '24';
const maxSourceBytes = 4 * 1024 * 1024;
const markdownFile = /\.(md|mdx|markdown)$/i;

interface RepoContextActions {
    isSession(uri: vscode.Uri): boolean;
    runSession(uri: vscode.Uri): Promise<void>;
}

interface RepoContextSelection {
    anchor: RepoContextAnchor;
    revision: string;
    links: RepoContextLink[];
}

interface RepoContextRequest {
    id: string;
    prompt: string;
    session: vscode.Uri;
    anchor: RepoContextAnchor;
}

function run(program: string, args: string[], cwd: string): Promise<string> {
    return new Promise((resolve, reject) => {
        execFile(program, args, { cwd, timeout: 10_000, maxBuffer: 2 * 1024 * 1024 }, (err, stdout, stderr) => {
            if (err) {
                const reason = err.code === 'ENOENT' ? `${program} is not available on the editor host's PATH`
                    : stderr.trim() || err.message;
                reject(new Error(reason));
            } else {
                resolve(stdout);
            }
        });
    });
}

async function tsift(root: string, args: string[]): Promise<unknown> {
    return JSON.parse(await run('tsift', args, root));
}

async function gitRoot(dir: string): Promise<string> {
    return fs.realpath((await run('git', ['rev-parse', '--show-toplevel'], dir)).trim());
}

async function checkedFile(root: string, file: string): Promise<string> {
    const candidate = pathInRepo(root, file);
    if (!candidate) throw new Error('The source is outside this repository.');
    const resolved = await fs.realpath(candidate);
    if (!pathInRepo(root, resolved) || await gitRoot(path.dirname(resolved)) !== root) {
        throw new Error('The source belongs to a different repository.');
    }
    return resolved;
}

function sourceDocument(file: string): vscode.TextDocument | undefined {
    return vscode.workspace.textDocuments.find(doc => doc.uri.scheme === 'file' && doc.uri.fsPath === file);
}

async function fingerprint(file: string): Promise<{ hash: string; lines: number }> {
    if (sourceDocument(file)?.isDirty) throw new Error('Save the source file before selecting its context.');
    const stat = await fs.stat(file);
    if (!stat.isFile() || stat.size > maxSourceBytes) throw new Error('The source is not a file under 4 MiB.');
    const bytes = await fs.readFile(file);
    return {
        hash: createHash('sha256').update(bytes).digest('hex'),
        lines: bytes.toString('utf8').split('\n').length,
    };
}

async function search(root: string, mode: RepoContextMode, query: string): Promise<RepoContextAnchor[]> {
    if (mode === 'docs' && markdownFile.test(query)) {
        const file = await checkedFile(root, query);
        return parseRepoContextAnchors(await tsift(root, [
            'markdown-ast', '--json', '--max-items', searchLimit, '--path', root, '--', file,
        ]), root, mode, file);
    }
    const args = ['search', '--json', '--limit', searchLimit, '--no-tagpath', '--path', root];
    if (mode === 'docs') args.push('--lang', 'markdown', '--kind', 'heading');
    return parseRepoContextAnchors(await tsift(root, [...args, '--', query]), root, mode);
}

async function documentLinks(file: string): Promise<vscode.DocumentLink[]> {
    const uri = vscode.Uri.file(file);
    await vscode.workspace.openTextDocument(uri);
    return await vscode.commands.executeCommand<vscode.DocumentLink[]>(
        'vscode.executeLinkProvider', uri, 30,
    ) ?? [];
}

async function sourceLinks(root: string, file: string, anchor: RepoContextAnchor): Promise<RepoContextLink[]> {
    await vscode.extensions.getExtension('vscode.markdown-language-features')?.activate();
    const links = await documentLinks(file);
    const seen = new Set<string>();
    const result: RepoContextLink[] = [];
    for (const link of links) {
        const absolute = link.target?.scheme === 'file' ? pathInRepo(root, link.target.fsPath) : undefined;
        const line = link.range.start.line + 1;
        if (!absolute || line < anchor.line || line > anchor.endLine) continue;
        const relative = path.relative(root, absolute);
        if (seen.has(relative)) continue;
        seen.add(relative);
        result.push({ file: relative, name: relative, line: 1, endLine: 1, relation: 'Links to' });
    }
    // A basename search produces candidates. Resolve their Markdown links
    // before reporting backlinks, since text and similarly named files match too.
    const candidates = parseRepoContextMarkdownFiles(await tsift(root, [
        'search', '--json', '--exact', '--lang', 'markdown', '--no-tagpath',
        '--limit', '8', '--path', root, '--', path.basename(file),
    ]), root).filter(candidate => candidate !== file);
    const backlinks = await Promise.allSettled(candidates.map(async candidate => {
        const referrer = await checkedFile(root, candidate);
        const link = (await documentLinks(referrer)).find(item =>
            item.target?.scheme === 'file' && path.resolve(item.target.fsPath) === file);
        if (!link) return undefined;
        const line = link.range.start.line + 1;
        const relative = path.relative(root, referrer);
        return { file: relative, name: relative, line, endLine: line, relation: 'Links to this file from' };
    }));
    for (const backlink of backlinks) {
        if (backlink.status === 'fulfilled') {
            if (backlink.value) result.push(backlink.value);
        } else {
            console.warn('[agent-doc] Repo Context Explorer could not inspect a backlink candidate', backlink.reason);
        }
    }
    return result.slice(0, 20);
}

async function selectAnchor(root: string, anchor: RepoContextAnchor): Promise<RepoContextSelection> {
    const file = await checkedFile(root, anchor.file);
    const before = await fingerprint(file);
    if (anchor.line > before.lines || anchor.endLine > before.lines) {
        throw new Error('The source location changed. Search again before selecting it.');
    }
    let links: RepoContextLink[];
    if (anchor.kind === 'heading') {
        // Rehydrate file-based sections from tsift's current Markdown AST.
        // Search results may have been projected from a different revision.
        const outline = await tsift(root, [
            'markdown-ast', '--json', '--max-items', '100', '--path', root, '--', file,
        ]);
        const current = parseRepoContextAnchors(outline, root, 'docs', file).find(
            item => item.name === anchor.name && item.line === anchor.line,
        );
        if (!current) throw new Error('The heading changed or is outside the retained outline. Search by file path to refresh.');
        anchor = current;
        links = await sourceLinks(root, file, anchor);
    } else {
        const detail = await tsift(root, ['explain', '--json', '--no-tagpath', '--limit', '6', '--path', root, '--', anchor.name]);
        const current = currentRepoContextDefinition(detail, root, anchor);
        if (!current) throw new Error('The symbol location changed. Search again before selecting it.');
        anchor = current;
        links = parseRepoContextCalls(detail, root, anchor);
    }
    if (anchor.endLine > before.lines || (await fingerprint(file)).hash !== before.hash) {
        throw new Error('The source changed while loading. Search again before selecting it.');
    }
    return { anchor, revision: before.hash, links };
}

async function openSource(root: string, location: SourceLocation): Promise<void> {
    const uri = vscode.Uri.file(await checkedFile(root, location.file));
    const document = await vscode.workspace.openTextDocument(uri);
    if (location.line > document.lineCount) throw new Error('The source location changed. Refresh the search.');
    const start = new vscode.Position(location.line - 1, 0);
    const end = document.lineAt(Math.min(location.endLine, document.lineCount) - 1).range.end;
    await vscode.window.showTextDocument(document, { viewColumn: vscode.ViewColumn.One, selection: new vscode.Range(start, end) });
}

async function appendQuestion(uri: vscode.Uri, question: string): Promise<void> {
    const document = await vscode.workspace.openTextDocument(uri);
    const editor = await vscode.window.showTextDocument(document, { viewColumn: vscode.ViewColumn.One, preview: false });
    if (document.isDirty) throw new Error('Save the session before adding a question.');
    const newline = document.eol === vscode.EndOfLine.CRLF ? '\r\n' : '\n';
    const insertion = planExchangePromptInsertion(document.getText(), question, newline);
    if (!insertion) throw new Error('Choose an agent-doc session with an exchange section.');
    const position = document.positionAt(insertion.offset);
    if (!await editor.edit(edit => edit.insert(position, insertion.text))) {
        throw new Error('The session changed before the question was added. Try again.');
    }
    if (!await document.save()) throw new Error('The question is in the editor. Save the session, then run Agent Doc.');
    editor.selection = new vscode.Selection(position, position);
    editor.revealRange(new vscode.Range(position, position));
}

async function revealSession(
    document: vscode.TextDocument,
    offset?: number,
): Promise<void> {
    const position = offset === undefined ? undefined : document.positionAt(offset);
    await vscode.window.showTextDocument(document, {
        viewColumn: vscode.ViewColumn.One,
        preview: false,
        selection: position && new vscode.Range(position, position),
    });
}

async function pickSession(root: string, actions: RepoContextActions): Promise<vscode.Uri | undefined> {
    const selected = await vscode.window.showOpenDialog({
        defaultUri: vscode.Uri.file(root),
        filters: { Markdown: ['md', 'mdx', 'markdown'] },
        openLabel: 'Choose Agent Doc session',
    });
    if (!selected?.[0]) return undefined;
    const uri = vscode.Uri.file(await checkedFile(root, selected[0].fsPath));
    if (!actions.isSession(uri)) throw new Error('Choose an initialized agent-doc session.');
    return uri;
}

function renderPanel(panel: vscode.WebviewPanel, media: vscode.Uri): string {
    const nonce = randomBytes(16).toString('hex');
    const css = panel.webview.asWebviewUri(vscode.Uri.joinPath(media, 'repoContext.css'));
    const script = panel.webview.asWebviewUri(vscode.Uri.joinPath(media, 'repoContext.js'));
    return `<!doctype html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${panel.webview.cspSource}; script-src 'nonce-${nonce}'">
    <link rel="stylesheet" href="${css}">
    <title>Repo Context Explorer</title>
</head>
<body>
    <header>
        <div><h1>Repo Context Explorer</h1><p id="repository"></p></div>
        <div class="session"><button id="open-session" type="button" hidden>Open session</button><button id="choose-session" class="secondary" type="button">Choose session</button></div>
    </header>
    <main>
        <section aria-label="Repository search">
            <form id="search-form">
                <fieldset><legend>Search scope</legend><label><input type="radio" name="mode" value="code" checked>Code</label><label><input type="radio" name="mode" value="docs">Docs</label></fieldset>
                <label for="query" id="query-label">Code symbol</label>
                <div class="field"><input id="query" type="search" required maxlength="200" placeholder="Function, class, or type"><button>Search</button></div>
            </form>
            <ol id="results" aria-label="Search results"></ol>
        </section>
        <section id="detail" aria-label="Selected source">
            <h2 id="source-name">Selected source</h2>
            <p id="source-location">Select a search result to inspect the source or ask the agent.</p>
            <button id="open-source" class="secondary" type="button" hidden>Open source</button>
            <ul id="related" aria-label="Related sources"></ul>
            <form id="ask-form" hidden>
                <label for="question">Ask about selection</label>
                <textarea id="question" rows="3" maxlength="1500" required></textarea>
                <button id="submit-question">Ask agent</button>
            </form>
        </section>
        <section id="answer" aria-live="polite" hidden>
            <div class="answer-header">
                <div><h2>Agent answer</h2><p id="answer-source"></p></div>
                <button id="open-answer" class="secondary" type="button">Open answer</button>
            </div>
            <pre id="answer-body"></pre>
        </section>
    </main>
    <p id="feedback" role="status" aria-live="polite"></p>
    <script nonce="${nonce}" src="${script}"></script>
</body>
</html>`;
}

export async function openRepoContextExplorer(
    extensionUri: vscode.Uri,
    actions: RepoContextActions,
): Promise<void> {
    if (!vscode.workspace.isTrusted) throw new Error('Trust this workspace to search it with tsift.');
    const active = vscode.window.activeTextEditor?.document.uri;
    const activeWorkspace = active && vscode.workspace.getWorkspaceFolder(active);
    const workspace = activeWorkspace
        ?? (vscode.workspace.workspaceFolders?.length === 1 ? vscode.workspace.workspaceFolders[0]
            : await vscode.window.showWorkspaceFolderPick({ placeHolder: 'Choose a repository workspace' }));
    if (!workspace) return;
    const root = await gitRoot(active?.scheme === 'file' && activeWorkspace === workspace
        ? path.dirname(active.fsPath) : workspace.uri.fsPath);
    const media = vscode.Uri.joinPath(extensionUri, 'media');
    const panel = vscode.window.createWebviewPanel(
        'agentDoc.repoContextExplorer', 'Repo Context Explorer', vscode.ViewColumn.Beside,
        { enableScripts: true, retainContextWhenHidden: true, localResourceRoots: [media] },
    );
    let session = active?.scheme === 'file' && actions.isSession(active) ? active : undefined;
    let anchors: RepoContextAnchor[] = [];
    let selection: RepoContextSelection | undefined;
    let latestRequest: RepoContextRequest | undefined;
    let busy = false;
    const post = (message: object) => panel.webview.postMessage(message);
    const showSession = () => post({ type: 'session', label: session && path.relative(root, session.fsPath) });
    const sessionSaved = vscode.workspace.onDidSaveTextDocument(document => {
        const request = latestRequest;
        if (!request || document.uri.toString() !== request.session.toString()) return;
        const answer = repoContextAnswer(document.getText(), request.id);
        const body = answer?.body;
        Promise.resolve(post({ type: 'answer', source: request.anchor,
            body: body && (body.length > 8000 ? body.slice(0, 8000) + '\n…' : body) }))
            .catch(error => console.warn('[agent-doc] Repo Context Explorer could not display the response', error));
    });
    panel.onDidDispose(() => sessionSaved.dispose());

    panel.webview.onDidReceiveMessage(async (message: {
        type?: unknown; mode?: unknown; query?: unknown; index?: unknown; question?: unknown;
    } | null) => {
        if (!message || typeof message.type !== 'string') return;
        if (message.type === 'ready') {
            await post({ type: 'repository', name: path.basename(root) });
            await showSession();
            return;
        }
        if (busy) return;
        busy = true;
        try {
            switch (message.type) {
                case 'search': {
                    if ((message.mode !== 'code' && message.mode !== 'docs') || typeof message.query !== 'string') return;
                    const query = message.query.trim();
                    if (!query || query.length > 200) return;
                    selection = undefined;
                    anchors = [];
                    await post({ type: 'results', items: [], loading: true });
                    anchors = await search(root, message.mode, query);
                    await post({ type: 'results', items: anchors });
                    break;
                }
                case 'select': {
                    const anchor = typeof message.index === 'number' ? anchors[message.index] : undefined;
                    if (!anchor) return;
                    selection = undefined;
                    await post({ type: 'selection', loading: true });
                    selection = await selectAnchor(root, anchor);
                    await post({ type: 'selection', item: selection.anchor, links: selection.links });
                    break;
                }
                case 'openSource':
                    if (selection) await openSource(root, selection.anchor);
                    break;
                case 'openLink': {
                    const link = typeof message.index === 'number' ? selection?.links[message.index] : undefined;
                    if (link) await openSource(root, link);
                    break;
                }
                case 'chooseSession':
                    session = await pickSession(root, actions) ?? session;
                    await showSession();
                    break;
                case 'openSession': {
                    if (!session) return;
                    const document = await vscode.workspace.openTextDocument(session);
                    const offset = latestRequest?.session.toString() === session.toString()
                        ? exchangeQuestionOffset(document.getText(), latestRequest.prompt) : undefined;
                    await revealSession(document, offset);
                    break;
                }
                case 'openAnswer': {
                    if (!latestRequest) return;
                    const document = await vscode.workspace.openTextDocument(latestRequest.session);
                    const answer = repoContextAnswer(document.getText(), latestRequest.id);
                    if (!answer) throw new Error('The matching response is no longer in the session exchange.');
                    await revealSession(document, answer.offset);
                    break;
                }
                case 'ask': {
                    if (!selection || typeof message.question !== 'string') return;
                    const question = message.question.trim();
                    if (!question || question.length > 1500) return;
                    session ??= await pickSession(root, actions);
                    if (!session) return;
                    await showSession();
                    await checkedFile(root, session.fsPath);
                    if (!actions.isSession(session)) throw new Error('The selected file is no longer an agent-doc session.');
                    const file = await checkedFile(root, selection.anchor.file);
                    if ((await fingerprint(file)).hash !== selection.revision) {
                        throw new Error('The selected source changed. Search again before asking.');
                    }
                    const id = randomBytes(16).toString('hex');
                    const prompt = formatRepoContextQuestion(selection.anchor, selection.revision, question, id);
                    await appendQuestion(session, prompt);
                    latestRequest = { id, prompt, session, anchor: selection.anchor };
                    await post({ type: 'asked', file: path.relative(root, session.fsPath) });
                    await actions.runSession(session);
                    break;
                }
            }
        } catch (error) {
            await post({ type: 'error', message: error instanceof Error ? error.message : String(error) });
        } finally {
            busy = false;
            await post({ type: 'settled' });
        }
    });
    panel.webview.html = renderPanel(panel, media);
}
