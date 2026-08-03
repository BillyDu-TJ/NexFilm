(function () {
    const controlStates = [];

    function closeAll(except) {
        controlStates.forEach(state => {
            if (state !== except) state.close();
        });
    }

    function placePopover(panel, control, trigger) {
        const dialog = control.closest('.import-dialog');
        if (!dialog || panel.classList.contains('hidden')) return;

        panel.style.inset = 'auto';
        const dialogRect = dialog.getBoundingClientRect();
        const controlRect = control.getBoundingClientRect();
        const triggerRect = trigger.getBoundingClientRect();
        const panelRect = panel.getBoundingClientRect();
        const topLimit = dialogRect.top + 68;
        const bottomLimit = dialogRect.bottom - 10;
        const below = triggerRect.bottom + 6;
        const above = triggerRect.top - panelRect.height - 6;
        let top = below + panelRect.height <= bottomLimit ? below : above;
        top = Math.max(topLimit, Math.min(top, bottomLimit - panelRect.height));
        let left = triggerRect.right - panelRect.width;
        left = Math.max(dialogRect.left + 10, Math.min(left, dialogRect.right - panelRect.width - 10));

        panel.style.top = (top - controlRect.top) + 'px';
        panel.style.left = (left - controlRect.left) + 'px';
    }

    function enhanceSelect(select) {
        if (!select || select.dataset.customControl === 'true') return;
        select.dataset.customControl = 'true';
        select.classList.add('native-control-proxy');

        const control = document.createElement('div');
        control.className = 'custom-select-control';
        select.parentNode.insertBefore(control, select);
        control.appendChild(select);

        const trigger = document.createElement('button');
        trigger.type = 'button';
        trigger.className = 'custom-select-trigger';
        trigger.setAttribute('aria-haspopup', 'listbox');
        trigger.setAttribute('aria-expanded', 'false');

        const menu = document.createElement('div');
        menu.className = 'custom-select-menu hidden';
        menu.setAttribute('role', 'listbox');
        control.append(trigger, menu);

        const state = {
            close() {
                menu.classList.add('hidden');
                trigger.setAttribute('aria-expanded', 'false');
            },
            sync() {
                const selected = select.selectedOptions[0] || select.options[0];
                trigger.textContent = selected ? selected.textContent : 'Select...';
                menu.replaceChildren();

                Array.from(select.children).forEach(child => {
                    const options = child.tagName === 'OPTGROUP'
                        ? Array.from(child.children)
                        : [child];
                    if (child.tagName === 'OPTGROUP') {
                        const heading = document.createElement('div');
                        heading.className = 'custom-select-group';
                        heading.textContent = child.label;
                        menu.appendChild(heading);
                    }
                    options.forEach(option => {
                        if (option.tagName !== 'OPTION') return;
                        const item = document.createElement('button');
                        item.type = 'button';
                        item.className = 'custom-select-option';
                        item.textContent = option.textContent;
                        item.setAttribute('role', 'option');
                        item.setAttribute('aria-selected', String(option.value === select.value));
                        item.addEventListener('click', () => {
                            select.value = option.value;
                            select.dispatchEvent(new Event('change', { bubbles: true }));
                            state.sync();
                            state.close();
                            trigger.focus();
                        });
                        menu.appendChild(item);
                    });
                });
            }
        };
        controlStates.push(state);

        trigger.addEventListener('click', () => {
            const opening = menu.classList.contains('hidden');
            closeAll(state);
            state.sync();
            menu.classList.toggle('hidden', !opening);
            trigger.setAttribute('aria-expanded', String(opening));
            if (opening) requestAnimationFrame(() => placePopover(menu, control, trigger));
        });
        trigger.addEventListener('keydown', event => {
            if (event.key === 'Escape') state.close();
            if (event.key === 'ArrowDown' && menu.classList.contains('hidden')) {
                event.preventDefault();
                trigger.click();
            }
        });
        new MutationObserver(() => state.sync()).observe(select, { childList: true, subtree: true });
        state.sync();
    }

    function displayDate(value) {
        return /^\d{4}-\d{2}-\d{2}$/.test(value) ? value.replaceAll('-', '/') : 'Select date';
    }

    function enhanceDate(input) {
        if (!input || input.dataset.customControl === 'true') return;
        input.dataset.customControl = 'true';
        input.classList.add('native-control-proxy');

        const control = document.createElement('div');
        control.className = 'custom-date-control';
        input.parentNode.insertBefore(control, input);
        control.appendChild(input);

        const trigger = document.createElement('button');
        trigger.type = 'button';
        trigger.className = 'custom-date-trigger';
        trigger.setAttribute('aria-haspopup', 'dialog');
        trigger.setAttribute('aria-expanded', 'false');

        const panel = document.createElement('div');
        panel.className = 'custom-date-panel hidden';
        const header = document.createElement('div');
        header.className = 'custom-date-header';
        const previous = document.createElement('button');
        previous.type = 'button';
        previous.className = 'custom-date-nav';
        previous.textContent = '‹';
        previous.setAttribute('aria-label', 'Previous month');
        const title = document.createElement('strong');
        const next = document.createElement('button');
        next.type = 'button';
        next.className = 'custom-date-nav';
        next.textContent = '›';
        next.setAttribute('aria-label', 'Next month');
        header.append(previous, title, next);

        const weekdays = document.createElement('div');
        weekdays.className = 'custom-date-weekdays';
        ['S', 'M', 'T', 'W', 'T', 'F', 'S'].forEach(day => {
            const label = document.createElement('span');
            label.textContent = day;
            weekdays.appendChild(label);
        });
        const grid = document.createElement('div');
        grid.className = 'custom-date-grid';
        panel.append(header, weekdays, grid);
        control.append(trigger, panel);

        const today = new Date();
        let shownYear = today.getFullYear();
        let shownMonth = today.getMonth();

        const state = {
            close() {
                panel.classList.add('hidden');
                trigger.setAttribute('aria-expanded', 'false');
            },
            sync(preserveView) {
                if (!preserveView && /^\d{4}-\d{2}-\d{2}$/.test(input.value)) {
                    const parts = input.value.split('-').map(Number);
                    shownYear = parts[0];
                    shownMonth = parts[1] - 1;
                }
                trigger.textContent = displayDate(input.value);
                title.textContent = new Intl.DateTimeFormat(undefined, {
                    month: 'long',
                    year: 'numeric'
                }).format(new Date(shownYear, shownMonth, 1));
                grid.replaceChildren();

                const leading = new Date(shownYear, shownMonth, 1).getDay();
                const days = new Date(shownYear, shownMonth + 1, 0).getDate();
                for (let index = 0; index < leading; index += 1) {
                    const spacer = document.createElement('span');
                    spacer.className = 'custom-date-spacer';
                    grid.appendChild(spacer);
                }
                for (let day = 1; day <= days; day += 1) {
                    const dateButton = document.createElement('button');
                    dateButton.type = 'button';
                    dateButton.className = 'custom-date-day';
                    dateButton.textContent = String(day);
                    const dateValue = String(shownYear) + '-'
                        + String(shownMonth + 1).padStart(2, '0') + '-'
                        + String(day).padStart(2, '0');
                    if (dateValue === input.value) dateButton.classList.add('is-selected');
                    dateButton.addEventListener('click', () => {
                        input.value = dateValue;
                        input.dispatchEvent(new Event('input', { bubbles: true }));
                        input.dispatchEvent(new Event('change', { bubbles: true }));
                        state.sync();
                        state.close();
                        trigger.focus();
                    });
                    grid.appendChild(dateButton);
                }
            }
        };
        controlStates.push(state);

        trigger.addEventListener('click', () => {
            const opening = panel.classList.contains('hidden');
            closeAll(state);
            state.sync(true);
            panel.classList.toggle('hidden', !opening);
            trigger.setAttribute('aria-expanded', String(opening));
            if (opening) requestAnimationFrame(() => placePopover(panel, control, trigger));
        });
        previous.addEventListener('click', () => {
            shownMonth -= 1;
            if (shownMonth < 0) {
                shownMonth = 11;
                shownYear -= 1;
            }
            state.sync(true);
        });
        next.addEventListener('click', () => {
            shownMonth += 1;
            if (shownMonth > 11) {
                shownMonth = 0;
                shownYear += 1;
            }
            state.sync();
        });
        input.addEventListener('change', () => state.sync());
        state.sync();
    }

    ['roll-format', 'roll-camera-select', 'roll-film-select', 'continue-roll-select']
        .forEach(id => enhanceSelect(document.getElementById(id)));
    enhanceDate(document.getElementById('roll-date'));

    document.addEventListener('pointerdown', event => {
        if (!event.target.closest('.custom-select-control, .custom-date-control')) closeAll();
    });
    window.addEventListener('resize', () => closeAll());

    window.NexFilmImportControls = {
        sync(close) {
            controlStates.forEach(state => {
                state.sync();
                if (close) state.close();
            });
        },
        close: closeAll
    };
})();
