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
    let pending = false;

    const send = (type, payload = {}) => {
        if (pending) return;
        pending = true;
        document.body.setAttribute('aria-busy', 'true');
        showFeedback('');
        vscode.postMessage({ type, ...payload });
    };

    function showFeedback(message, isError = false) {
        feedback.textContent = message;
        feedback.classList.toggle('error', isError);
    }

    function location(item) {
        const range = item.endLine === item.line ? item.line : `${item.line}–${item.endLine}`;
        return `${item.file}:${range}`;
    }

    function sourceRow(item, open) {
        const li = document.createElement('li');
        const button = document.createElement('button');
        button.className = 'item';
        button.type = 'button';
        const title = document.createElement('span');
        title.textContent = item.relation ? `${item.relation} · ${item.name}` : item.name;
        const subtitle = document.createElement('span');
        subtitle.className = 'location';
        subtitle.textContent = location(item);
        button.append(title, subtitle);
        button.addEventListener('click', open);
        li.appendChild(button);
        return li;
    }

    function select(index) {
        if (pending) return;
        Array.from(results.querySelectorAll('button')).forEach((row, position) => {
            row.setAttribute('aria-current', String(position === index));
        });
        send('select', { index });
    }

    function renderSelection(item, links = [], loading = false) {
        detail.setAttribute('aria-busy', String(loading));
        get('source-name').textContent = item ? item.name : 'Selected source';
        get('source-location').textContent = item ? location(item)
            : loading ? 'Loading source…' : 'Select a search result to inspect the source or ask the agent.';
        get('open-source').hidden = !item;
        askForm.hidden = !item;
        related.replaceChildren(...links.map((link, index) =>
            sourceRow(link, () => send('openLink', { index }))));
    }

    get('search-form').addEventListener('submit', event => {
        event.preventDefault();
        if (!query.value.trim()) return;
        const mode = document.querySelector('input[name="mode"]:checked').value;
        send('search', { mode, query: query.value });
    });
    for (const input of document.querySelectorAll('input[name="mode"]')) {
        input.addEventListener('change', () => {
            const docs = input.value === 'docs';
            get('query-label').textContent = docs ? 'Markdown section or path' : 'Code symbol';
            query.placeholder = docs ? 'Heading words or docs/guide.md' : 'Function, class, or type';
        });
    }
    get('open-session').addEventListener('click', () => send('openSession'));
    get('open-answer').addEventListener('click', () => send('openAnswer'));
    get('choose-session').addEventListener('click', () => send('chooseSession'));
    get('open-source').addEventListener('click', () => send('openSource'));
    askForm.addEventListener('submit', event => {
        event.preventDefault();
        if (question.value.trim()) send('ask', { question: question.value });
    });

    window.addEventListener('message', event => {
        const message = event.data;
        switch (message.type) {
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
                results.replaceChildren(...message.items.map((item, index) =>
                    sourceRow(item, () => select(index))));
                renderSelection();
                showFeedback(message.loading ? 'Searching…' : message.items.length ? '' : 'No matching sources.');
                break;
            case 'selection':
                renderSelection(message.item, message.links, message.loading);
                break;
            case 'asked':
                question.value = '';
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
                pending = false;
                document.body.removeAttribute('aria-busy');
                break;
        }
    });

    vscode.postMessage({ type: 'ready' });
})();
