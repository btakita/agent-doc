(() => {
    const vscode = acquireVsCodeApi();
    const get = id => document.getElementById(id);
    const query = get('query');
    const results = get('results');
    const related = get('related');
    const detail = get('detail');
    const question = get('question');
    const feedback = get('feedback');
    const askForm = get('ask-form');
    const askButton = get('submit-question');
    const resultRows = new Map();
    const relatedRows = new Map();
    const composing = new Set();
    let resultItems = [];
    let relatedItems = [];
    let selectedId;
    let selectedSource;
    let currentView;
    let pendingFrame = vscode.getState()?.pendingViewEvent;
    let submittedDraft;
    let pending = false;

    function setPending(value) {
        pending = value;
        if (value) document.body.setAttribute('aria-busy', 'true');
        else document.body.removeAttribute('aria-busy');
    }

    function rememberFrame(frame) {
        pendingFrame = frame;
        vscode.setState({ pendingViewEvent: frame });
    }

    function send(type, legacy = {}, { nodeId = type, event = 'activate', payload = null } = {}) {
        if (pending) return;
        setPending(true);
        showFeedback('');
        if (!currentView) {
            vscode.postMessage({ type, ...legacy });
            return;
        }
        const frame = {
            version: 1,
            view_instance: currentView.view_instance,
            displayed_revision: currentView.revision,
            node_id: nodeId,
            event_id: crypto.getRandomValues(new Uint32Array(4)).join('-'),
            event,
            payload,
        };
        rememberFrame(frame);
        vscode.postMessage({ type: 'view-event', frame });
    }

    function applyView(revision) {
        if (!revision?.definition?.nodes) return;
        if (currentView?.view_instance === revision.view_instance && revision.revision <= currentView.revision) return;
        const attached = currentView?.view_instance !== revision.view_instance;
        const replaced = attached && currentView !== undefined;
        currentView = revision;
        if (replaced) {
            resultItems = [];
            renderSelection();
        }
        renderResults();
        renderRelated();
        if (!attached || !pendingFrame) return;
        if (pendingFrame.view_instance !== revision.view_instance) {
            rememberFrame(null);
            setPending(false);
            return;
        }
        setPending(true);
        vscode.postMessage({ type: 'view-event', frame: pendingFrame });
    }

    function showFeedback(message, isError = false) {
        feedback.textContent = message;
        feedback.classList.toggle('error', isError);
    }

    function location(item) {
        const range = item.endLine === item.line ? item.line : `${item.line}–${item.endLine}`;
        return `${item.file}:${range}`;
    }

    function rowId(item) {
        return item.id ?? JSON.stringify([item.file, item.line, item.endLine, item.name, item.relation]);
    }

    function sourceRow(open) {
        const li = document.createElement('li');
        const button = document.createElement('button');
        button.className = 'item';
        button.type = 'button';
        const title = document.createElement('span');
        const subtitle = document.createElement('span');
        subtitle.className = 'location';
        button.append(title, subtitle);
        button.addEventListener('click', open);
        li.appendChild(button);
        return { li, button, title, subtitle };
    }

    function renderRows(list, rows, items, parentId, open) {
        const itemsById = new Map(items.map(item => [rowId(item), item]));
        const ids = currentView?.definition.nodes[parentId]?.children ?? itemsById.keys();
        const visible = new Set();
        let next = list.firstElementChild;
        for (const id of ids) {
            const item = itemsById.get(id);
            if (!item || visible.has(id)) continue;
            visible.add(id);
            let row = rows.get(id);
            if (!row) {
                row = sourceRow(() => open(id));
                rows.set(id, row);
            }
            const title = item.relation ? `${item.relation} · ${item.name}` : item.name;
            const subtitle = location(item);
            if (row.title.textContent !== title) row.title.textContent = title;
            if (row.subtitle.textContent !== subtitle) row.subtitle.textContent = subtitle;
            if (row.li !== next) {
                if (row.li.parentElement === list && typeof list.moveBefore === 'function') {
                    list.moveBefore(row.li, next);
                } else {
                    const focus = document.activeElement === row.button;
                    list.insertBefore(row.li, next);
                    if (focus) row.button.focus({ preventScroll: true });
                }
            }
            next = row.li.nextElementSibling;
        }
        for (const [id, row] of rows) {
            if (visible.has(id)) continue;
            row.li.remove();
            rows.delete(id);
        }
    }

    function highlightSelection() {
        for (const [id, row] of resultRows) {
            row.button.setAttribute('aria-current', String(id === selectedId));
        }
    }

    function select(id) {
        if (pending) return;
        const index = resultItems.findIndex(item => rowId(item) === id);
        selectedId = id;
        highlightSelection();
        send('select', { index }, { nodeId: id });
    }

    function renderResults() {
        renderRows(results, resultRows, resultItems, 'results', select);
        highlightSelection();
    }

    function renderRelated() {
        renderRows(related, relatedRows, relatedItems, 'related', id => {
            const index = relatedItems.findIndex(item => rowId(item) === id);
            send('openLink', { index }, { nodeId: id });
        });
    }

    function renderSelection(item, links = [], loading = false) {
        const refreshing = item && selectedSource && rowId(item) === rowId(selectedSource) && loading;
        selectedSource = item;
        if (item) selectedId = rowId(item);
        else if (!loading) selectedId = undefined;
        detail.setAttribute('aria-busy', String(loading));
        get('source-name').textContent = item ? item.name : 'Selected source';
        get('source-location').textContent = item ? location(item)
            : loading ? 'Loading source…' : 'Select a search result to inspect the source or ask the agent.';
        get('open-source').hidden = !item;
        askForm.hidden = !item && question.value === '' && document.activeElement !== question;
        askButton.disabled = !item;
        if (!refreshing || links.length) relatedItems = links;
        highlightSelection();
        renderRelated();
    }

    for (const input of [query, question]) {
        input.addEventListener('compositionstart', () => composing.add(input));
        input.addEventListener('compositionend', () => composing.delete(input));
    }
    get('search-form').addEventListener('submit', event => {
        event.preventDefault();
        if (composing.has(query) || !query.value.trim()) return;
        const mode = document.querySelector('input[name="mode"]:checked').value;
        const parameters = { mode, query: query.value };
        send('search', parameters, { event: 'submit', payload: JSON.stringify(parameters) });
    });
    for (const input of document.querySelectorAll('input[name="mode"]')) {
        input.addEventListener('change', () => {
            const docs = input.value === 'docs';
            get('query-label').textContent = docs ? 'Markdown section or path' : 'Code symbol';
            query.placeholder = docs ? 'Heading words or docs/guide.md' : 'Function, class, or type';
        });
    }
    get('open-session').addEventListener('click', () => send('openSession', {}, { nodeId: 'open-session' }));
    get('open-answer').addEventListener('click', () => send('openAnswer', {}, { nodeId: 'open-answer' }));
    get('choose-session').addEventListener('click', () => send('chooseSession', {}, { nodeId: 'choose-session' }));
    get('open-source').addEventListener('click', () => send('openSource', {}, { nodeId: 'open-source' }));
    askForm.addEventListener('submit', event => {
        event.preventDefault();
        if (pending || !selectedSource || composing.has(question) || !question.value.trim()) return;
        submittedDraft = question.value;
        send('ask', { question: submittedDraft }, { nodeId: 'submit-question', event: 'submit', payload: submittedDraft });
    });

    window.addEventListener('message', event => {
        const message = event.data;
        switch (message.type) {
            case 'view':
                applyView(message.revision);
                break;
            case 'view-values':
                if (Number.isSafeInteger(message.relatedCount) && message.relatedCount >= 0) {
                    related.setAttribute('aria-label', `Related sources (${message.relatedCount})`);
                }
                break;
            case 'repository':
                get('repository').textContent = message.name;
                break;
            case 'session': {
                const showSessionButton = !!message.label;
                const open = get('open-session');
                open.hidden = !showSessionButton;
                open.textContent = showSessionButton ? `Open session · ${message.label}` : '';
                get('choose-session').textContent = showSessionButton ? 'Change session' : 'Choose session';
                break;
            }
            case 'results':
                if (message.loading) {
                    showFeedback('Searching…');
                    break;
                }
                resultItems = message.items;
                renderResults();
                if (selectedId && !resultItems.some(item => rowId(item) === selectedId)) renderSelection();
                showFeedback(resultItems.length ? '' : 'No matching sources.');
                break;
            case 'selection':
                renderSelection(message.item, message.links, message.loading);
                break;
            case 'asked':
                if (message.event_id !== undefined && pendingFrame?.event_id !== message.event_id) break;
                if (question.value === submittedDraft && !composing.has(question)) question.value = '';
                get('answer').hidden = true;
                showFeedback(`Question saved in ${message.file}. Follow the agent’s response in the session.`);
                break;
            case 'answer':
                get('answer').hidden = !message.body;
                if (message.body) {
                    get('answer-source').textContent = location(message.source);
                    get('answer-body').textContent = message.body;
                    showFeedback('');
                }
                break;
            case 'error':
                if (detail.getAttribute('aria-busy') === 'true') renderSelection();
                showFeedback(message.message, true);
                break;
            case 'settled':
                if (pendingFrame && pendingFrame.event_id !== message.event_id) break;
                if (pendingFrame) rememberFrame(null);
                submittedDraft = undefined;
                setPending(false);
                break;
        }
    });

    vscode.postMessage({ type: 'ready' });
})();
