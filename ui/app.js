/**
 * Moneta - Obsidian-Style Local Knowledge Vault
 * 100% Offline • No AI APIs Used
 */

// Helper to check if Tauri API is present
const hasTauri = typeof window.__TAURI__ !== 'undefined' || typeof window.__TAURI_INTERNALS__ !== 'undefined';

async function tauriInvoke(cmd, args = {}) {
  if (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke) {
    return window.__TAURI__.core.invoke(cmd, args);
  }
  if (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) {
    return window.__TAURI_INTERNALS__.invoke(cmd, args);
  }
  console.log(`[Mock Tauri Invoke] ${cmd}`, args);
  return mockBackend(cmd, args);
}

// -------------------------------------------------------------
// SEED DATA (For standalone / initial rendering matching user pics)
// -------------------------------------------------------------
const INITIAL_WRITING_NOTES = [
  { id: 'writing-acceptance', title: 'Acceptance', folder: 'Writing', content: 'On acceptance and letting go of unnecessary expectations.\n\nAcceptance is not resignation; it is meeting reality on its own terms.' },
  { id: 'writing-courage', title: 'Courage', folder: 'Writing', content: 'Courage is resistance to fear, mastery of fear, not absence of fear.' },
  { id: 'writing-false-sheriffs', title: 'False Sheriffs', folder: 'Writing', content: 'False authorities and self-appointed gatekeepers.\n\nObserving how power structures manifest in small community dynamics.' },
  { id: 'writing-hate', title: 'Hate', folder: 'Writing', content: 'Reflections on hate and constructive emotional redirection.' },
  { id: 'writing-jealousy', title: 'Jealousy', folder: 'Writing', content: 'The roots of envy and focusing on intrinsic fulfillment.' },
  { id: 'writing-leisure', title: 'Leisure', folder: 'Writing', content: 'Leisure as the basis of culture and deep thought.' },
  { id: 'writing-pain', title: 'Pain', folder: 'Writing', content: 'Physical pain versus existential discomfort. Patterns of resilience.' },
  { id: 'writing-patience', title: 'Patience, Irritation and Love', folder: 'Writing', content: 'Patience is the calm acceptance that things happen in a different order than the one you have in mind.' },
  { id: 'writing-strength', title: 'Strength and cowardice', folder: 'Writing', content: 'Strength in restraint; the subtle boundaries between bravery and recklessness.' },
  { id: 'writing-uncertainty', title: 'Uncertainty and fragility', folder: 'Writing', content: 'Embracing antifragility in high-variance environments.' }
];

const INITIAL_QUICK_NOTES = [
  { id: 'note-2026-10-13', title: '2026-10-13', folder: 'Quick Notes', content: '# 2026-10-13\n\nDaily log. Reviewing Moneta architecture and Obsidian UI design.' },
  { id: 'note-college', title: 'College', folder: 'Quick Notes', content: 'Semester project outlines and study schedules.' },
  {
    id: 'note-draft',
    title: 'draft',
    folder: 'Writing',
    content: 'Hmm, there are all sorts of arranged marriages ofc. I mean, the landscape has evolved a lot these days though.\n\nPeople have this concept of dating to marry, where people basically court each other for 6 months plus and then marry. Some rural places, it still is phone calls and pictures and 1 meeting with the families so there is that too.\n\nThe '
  },
  { id: 'note-git', title: 'GIT', folder: 'Quick Notes', content: 'Git workflows, rebase conventions, and submodules guide.' },
  { id: 'note-ideas', title: 'Ideas', folder: 'Quick Notes', content: 'Product concepts for local-first knowledge management.' },
  { id: 'note-keerthan-test-1', title: 'Keerthan test 1', folder: 'Quick Notes', content: '# Keerthan test 1\n\nConnected to [[Chapter 1]]. Testing bidirectional references and knowledge graph visualization.' },
  { id: 'note-chapter-1', title: 'Chapter 1', folder: 'Quick Notes', content: '# Chapter 1\n\nDirect link to [[Keerthan test 1]]. Exploring network node gravity and semantic clustering.' },
  { id: 'note-mac-address', title: 'Mac Address', folder: 'Quick Notes', content: 'Hardware address notes and network protocol definitions.' },
  { id: 'note-msme', title: 'MSME Idea Hackathon', folder: 'Quick Notes', content: 'Notes and pitches for the MSME Hackathon submission.' },
  { id: 'note-orchard', title: 'Orchard', folder: 'Quick Notes', content: 'Orchard concept notes and system architecture.' },
  { id: 'note-people', title: 'People', folder: 'Quick Notes', content: 'Directory of collaborators and contacts.' },
  { id: 'note-untitled-canvas', title: 'Untitled', badge: 'CANVAS', folder: 'Quick Notes', content: 'Canvas layout and visual node mapping.' },
  { id: 'note-untitled', title: 'Untitled', folder: 'Quick Notes', content: 'Quick scratchpad note.' }
];

// App State
let state = {
  currentTabId: 'draft',
  openTabs: [
    { id: 'orchard', title: 'Orchard' },
    { id: 'people', title: 'People' },
    { id: 'draft', title: 'draft' },
    { id: 'keerthan-test-1', title: 'Keerthan test 1' },
    { id: 'graph-view', title: 'Graph view', isGraph: true }
  ],
  vaultItems: [],
  notesById: new Map(),
  graphData: null,
  isEditMode: false
};

// -------------------------------------------------------------
// 1. UNIVERSE ZOOM INTRO ANIMATION
// -------------------------------------------------------------
function initUniverseAnimation() {
  const canvas = document.getElementById('universe-canvas');
  const loader = document.getElementById('universe-loading-screen');
  const skipBtn = document.getElementById('skip-intro-btn');
  if (!canvas || !loader) return;

  const ctx = canvas.getContext('2d');
  let width = (canvas.width = window.innerWidth);
  let height = (canvas.height = window.innerHeight);

  window.addEventListener('resize', () => {
    width = canvas.width = window.innerWidth;
    height = canvas.height = window.innerHeight;
  });

  const numStars = 450;
  const stars = [];
  for (let i = 0; i < numStars; i++) {
    stars.push({
      x: (Math.random() - 0.5) * width * 2,
      y: (Math.random() - 0.5) * height * 2,
      z: Math.random() * width,
      oZ: Math.random() * width,
      color: Math.random() > 0.8 ? '#c792ea' : Math.random() > 0.6 ? '#61c5ff' : '#ffffff',
      size: Math.random() * 2 + 1
    });
  }

  let startTime = performance.now();
  let animationEnded = false;

  function endAnimation() {
    if (animationEnded) return;
    animationEnded = true;
    loader.classList.add('fade-out');
    setTimeout(() => {
      loader.style.display = 'none';
    }, 850);
  }

  skipBtn.addEventListener('click', endAnimation);
  loader.addEventListener('click', (e) => {
    if (e.target !== skipBtn) endAnimation();
  });

  // Automatically fade out after 2.4 seconds
  setTimeout(endAnimation, 2400);

  function render(now) {
    if (animationEnded) return;

    ctx.fillStyle = 'rgba(5, 6, 8, 0.28)';
    ctx.fillRect(0, 0, width, height);

    const elapsed = (now - startTime) / 1000;
    const speed = Math.min(28, 4 + elapsed * 18); // smooth acceleration zoom

    const cx = width / 2;
    const cy = height / 2;

    for (const star of stars) {
      star.z -= speed;
      if (star.z <= 0) {
        star.z = width;
        star.x = (Math.random() - 0.5) * width * 2;
        star.y = (Math.random() - 0.5) * height * 2;
      }

      const k = 300 / star.z;
      const px = star.x * k + cx;
      const py = star.y * k + cy;

      if (px >= 0 && px <= width && py >= 0 && py <= height) {
        const size = Math.max(0.5, (1 - star.z / width) * star.size * 2.5);
        const alpha = Math.min(1, (1 - star.z / width) * 1.5);
        ctx.beginPath();
        ctx.arc(px, py, size, 0, Math.PI * 2);
        ctx.fillStyle = star.color;
        ctx.globalAlpha = alpha;
        ctx.fill();
        ctx.globalAlpha = 1;
      }
    }

    requestAnimationFrame(render);
  }

  requestAnimationFrame(render);
}

// -------------------------------------------------------------
// 2. MOCK BACKEND (Fallback for browser preview / testing)
// -------------------------------------------------------------
function mockBackend(cmd, args) {
  if (cmd === 'get_vault_structure') {
    const bms = [];
    return {
      folders: ['Writing', 'Web Bookmarks', 'Images & OCR', 'Documents', 'Quick Notes'],
      bookmarks: bms
    };
  }
  if (cmd === 'get_graph_data') {
    const nodes = [
      { id: 'draft', title: 'draft', source_type: 'note', tags: ['writing'], summary: 'Arranged marriages essay draft' },
      { id: 'keerthan-test-1', title: 'Keerthan test 1', source_type: 'note', tags: ['test'], summary: 'Test note with chapter reference' },
      { id: 'chapter-1', title: 'Chapter 1', source_type: 'note', tags: ['book'], summary: 'First chapter connected to Keerthan' },
      { id: 'orchard', title: 'Orchard', source_type: 'note', tags: ['project'], summary: 'Knowledge orchard notes' },
      { id: 'people', title: 'People', source_type: 'note', tags: ['contacts'], summary: 'Directory of people' },
      { id: 'msme', title: 'MSME Idea Hackathon', source_type: 'note', tags: ['hackathon'], summary: 'Idea pitch document' },
      { id: 'git', title: 'GIT', source_type: 'note', tags: ['dev'], summary: 'Git commands reference' },
      { id: 'mac-address', title: 'Mac Address', source_type: 'note', tags: ['network'], summary: 'Hardware addressing notes' },
      { id: 'courage', title: 'Courage', source_type: 'note', tags: ['philosophy'], summary: 'Thoughts on fear and action' },
      { id: 'pain', title: 'Pain', source_type: 'note', tags: ['health'], summary: 'Reflections on endurance' }
    ];
    const edges = [
      { source: 'keerthan-test-1', target: 'chapter-1', similarity: null },
      { source: 'draft', target: 'people', similarity: 0.65 },
      { source: 'orchard', target: 'draft', similarity: 0.42 },
      { source: 'courage', target: 'pain', similarity: 0.55 },
      { source: 'msme', target: 'git', similarity: 0.38 }
    ];
    return { nodes, edges, search_history: [], search_history_status: 'ok' };
  }
  if (cmd === 'search_bookmarks') {
    return [];
  }
  return {};
}

// -------------------------------------------------------------
// 3. VAULT DATA LOADING & TREE VIEW
// -------------------------------------------------------------
async function loadVault() {
  // Prepopulate initial notes into memory
  [...INITIAL_WRITING_NOTES, ...INITIAL_QUICK_NOTES].forEach(n => {
    state.notesById.set(n.id, n);
  });

  try {
    const res = await tauriInvoke('get_vault_structure');
    if (res && res.bookmarks && res.bookmarks.length > 0) {
      res.bookmarks.forEach(bm => {
        state.notesById.set(bm.id, {
          id: bm.id,
          title: bm.title,
          folder: bm.folder || 'Web Bookmarks',
          content: bm.content_preview || bm.summary || '',
          source_url: bm.source_url,
          tags: bm.tags,
          created_at: bm.created_at,
          source_type: bm.source_type
        });
      });
    }
  } catch (err) {
    console.warn('Vault load error:', err);
  }

  renderSidebarTree();
}

function renderSidebarTree(filterText = '') {
  const container = document.getElementById('vault-tree');
  if (!container) return;

  const filter = filterText.toLowerCase().trim();
  const allNotes = Array.from(state.notesById.values());

  // Group notes into folders
  const groups = new Map();
  groups.set('Writing', []);
  groups.set('Web Bookmarks', []);
  groups.set('Images & OCR', []);
  groups.set('Documents', []);
  groups.set('Quick Notes', []);

  for (const note of allNotes) {
    if (filter) {
      const matchTitle = note.title.toLowerCase().includes(filter);
      const matchTags = (note.tags || []).some(t => t.toLowerCase().includes(filter));
      if (!matchTitle && !matchTags) continue;
    }

    const fName = groups.has(note.folder) ? note.folder : 'Quick Notes';
    groups.get(fName).push(note);
  }

  container.innerHTML = '';

  for (const [folderName, items] of groups.entries()) {
    if (filter && items.length === 0) continue;

    const folderEl = document.createElement('div');
    folderEl.className = 'tree-folder open';

    const headerEl = document.createElement('div');
    headerEl.className = 'tree-folder-header';
    headerEl.innerHTML = `
      <span class="folder-arrow">&#9658;</span>
      <span class="folder-name">${folderName}</span>
      <span class="folder-count">${items.length}</span>
    `;

    headerEl.addEventListener('click', () => {
      folderEl.classList.toggle('open');
    });

    const childrenEl = document.createElement('div');
    childrenEl.className = 'tree-folder-children';

    items.forEach(note => {
      const itemEl = document.createElement('div');
      itemEl.className = `tree-item ${state.currentTabId === note.id ? 'active' : ''}`;
      itemEl.dataset.noteId = note.id;

      let badgeHtml = note.badge ? `<span class="tree-item-badge">${note.badge}</span>` : '';
      if (!badgeHtml && note.source_type && note.source_type !== 'note') {
        badgeHtml = `<span class="tree-item-badge">${note.source_type}</span>`;
      }

      itemEl.innerHTML = `
        <span class="tree-item-title">${escapeHtml(note.title)}</span>
        ${badgeHtml}
      `;

      itemEl.addEventListener('click', () => {
        openNote(note.id);
      });

      childrenEl.appendChild(itemEl);
    });

    folderEl.appendChild(headerEl);
    folderEl.appendChild(childrenEl);
    container.appendChild(folderEl);
  }
}

// -------------------------------------------------------------
// 4. NOTE OPENING & EDITOR VIEW
// -------------------------------------------------------------
function openNote(noteId) {
  let note = state.notesById.get(noteId);
  if (!note) {
    note = { id: noteId, title: noteId, content: `# ${noteId}\n\n`, folder: 'Quick Notes' };
    state.notesById.set(noteId, note);
  }

  // Ensure tab exists
  const existingTab = state.openTabs.find(t => t.id === noteId);
  if (!existingTab) {
    state.openTabs.push({ id: noteId, title: note.title });
  }

  state.currentTabId = noteId;
  renderTabs();
  showEditorView();

  // Update Breadcrumbs
  document.getElementById('doc-breadcrumb-folder').textContent = note.folder || 'Vault';
  document.getElementById('doc-breadcrumb-title').textContent = note.title;

  // Title
  const titleEl = document.getElementById('document-title');
  titleEl.textContent = note.title;

  // Metadata Card
  const metaCard = document.getElementById('document-meta-card');
  const sourceLink = document.getElementById('meta-source-link');
  const tagsContainer = document.getElementById('meta-tags-container');
  const dateEl = document.getElementById('meta-date');

  if (note.source_url || (note.tags && note.tags.length > 0)) {
    metaCard.classList.remove('hidden');
    if (note.source_url) {
      sourceLink.textContent = note.source_url;
      sourceLink.href = note.source_url;
      sourceLink.parentElement.style.display = 'flex';
    } else {
      sourceLink.parentElement.style.display = 'none';
    }

    tagsContainer.innerHTML = '';
    (note.tags || []).forEach(t => {
      const pill = document.createElement('span');
      pill.className = 'tag-pill';
      pill.textContent = `#${t}`;
      tagsContainer.appendChild(pill);
    });

    dateEl.textContent = note.created_at ? new Date(note.created_at * 1000).toLocaleString() : 'Recently captured';
  } else {
    metaCard.classList.add('hidden');
  }

  // Content
  const readerEl = document.getElementById('note-reader-content');
  const editorEl = document.getElementById('note-editor-content');

  readerEl.innerHTML = renderMarkdown(note.content);
  editorEl.value = note.content;

  updateWordCounts(note.content);
  renderSidebarTree(document.getElementById('sidebar-filter-input').value);
}

function showEditorView() {
  document.getElementById('view-editor').classList.add('active');
  document.getElementById('view-graph').classList.remove('active');
}

function showGraphView() {
  document.getElementById('view-editor').classList.remove('active');
  document.getElementById('view-graph').classList.add('active');

  // Activate Graph tab
  state.currentTabId = 'graph-view';
  const existing = state.openTabs.find(t => t.id === 'graph-view');
  if (!existing) {
    state.openTabs.push({ id: 'graph-view', title: 'Graph view', isGraph: true });
  }
  renderTabs();
  loadAndRenderGraph();
}

function updateWordCounts(text = '') {
  const chars = text.length;
  const words = text.trim() ? text.trim().split(/\s+/).length : 0;

  document.getElementById('status-word-count').textContent = `${words} words`;
  document.getElementById('status-char-count').textContent = `${chars} characters`;
}

// -------------------------------------------------------------
// 5. TABS MANAGEMENT
// -------------------------------------------------------------
function renderTabs() {
  const container = document.getElementById('tabs-container');
  if (!container) return;

  const addBtn = document.getElementById('btn-add-tab');
  container.innerHTML = '';

  state.openTabs.forEach(tab => {
    const tabEl = document.createElement('div');
    tabEl.className = `tab-item ${state.currentTabId === tab.id ? 'active' : ''}`;
    tabEl.dataset.tabId = tab.id;

    let iconHtml = '';
    if (tab.isGraph) {
      iconHtml = `<span class="tab-icon"><svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="6" cy="6" r="3"/><circle cx="18" cy="18" r="3"/><circle cx="6" cy="18" r="3"/><line x1="8.5" y1="7.5" x2="16" y2="16"/></svg></span>`;
    }

    tabEl.innerHTML = `
      ${iconHtml}
      <span class="tab-title">${escapeHtml(tab.title)}</span>
      <span class="tab-close" data-close-id="${tab.id}">&times;</span>
    `;

    tabEl.addEventListener('click', (e) => {
      if (e.target.classList.contains('tab-close')) {
        closeTab(tab.id);
        return;
      }
      if (tab.isGraph) {
        showGraphView();
      } else {
        openNote(tab.id);
      }
    });

    container.appendChild(tabEl);
  });

  if (addBtn) container.appendChild(addBtn);
}

function closeTab(tabId) {
  state.openTabs = state.openTabs.filter(t => t.id !== tabId);
  if (state.openTabs.length === 0) {
    state.openTabs.push({ id: 'draft', title: 'draft' });
  }
  if (state.currentTabId === tabId) {
    const nextTab = state.openTabs[state.openTabs.length - 1];
    if (nextTab.isGraph) {
      showGraphView();
    } else {
      openNote(nextTab.id);
    }
  } else {
    renderTabs();
  }
}

// -------------------------------------------------------------
// 6. KNOWLEDGE GRAPH VIEW (Matching Image 2)
// -------------------------------------------------------------
let graphSimulation = null;

async function loadAndRenderGraph() {
  const canvas = document.getElementById('knowledge-graph-canvas');
  if (!canvas) return;

  const ctx = canvas.getContext('2d');
  let width = (canvas.width = canvas.parentElement.clientWidth);
  let height = (canvas.height = canvas.parentElement.clientHeight);

  window.addEventListener('resize', () => {
    width = canvas.width = canvas.parentElement.clientWidth;
    height = canvas.height = canvas.parentElement.clientHeight;
  });

  let rawData = null;
  try {
    rawData = await tauriInvoke('get_graph_data');
  } catch (e) {
    console.warn('Could not fetch graph data, using fallback:', e);
  }

  if (!rawData || !rawData.nodes || rawData.nodes.length === 0) {
    rawData = mockBackend('get_graph_data');
  }

  // Populate graph nodes with force physics variables
  const nodeCount = rawData.nodes.length;
  const nodes = rawData.nodes.map((n, i) => {
    const angle = i * 2.399;
    const r = Math.min(width, height) * 0.28 * Math.sqrt((i + 1) / nodeCount);
    return {
      ...n,
      x: Math.cos(angle) * r,
      y: Math.sin(angle) * r,
      vx: 0,
      vy: 0,
      radius: n.id === 'draft' || n.id === 'keerthan-test-1' || n.id === 'chapter-1' ? 7.5 : 5
    };
  });

  const nodeMap = new Map(nodes.map(n => [n.id, n]));
  const edges = (rawData.edges || []).filter(e => nodeMap.has(e.source) && nodeMap.has(e.target));

  // Build adjacency
  const adj = new Map(nodes.map(n => [n.id, []]));
  for (const e of edges) {
    adj.get(e.source).push({ target: nodeMap.get(e.target), edge: e });
    adj.get(e.target).push({ target: nodeMap.get(e.source), edge: e });
  }

  let scale = 1.0;
  let offsetX = width / 2;
  let offsetY = height / 2;
  let isDragging = false;
  let dragNode = null;
  let hoveredNode = null;
  let lastMouse = { x: 0, y: 0 };

  const tooltip = document.getElementById('graph-node-tooltip');
  const searchInput = document.getElementById('graph-search-input');
  const sliderDist = document.getElementById('slider-link-dist');
  const sliderRep = document.getElementById('slider-repulsion');
  const sliderSize = document.getElementById('slider-node-size');
  const chkLabels = document.getElementById('chk-show-tags');

  // Simulation loop
  if (graphSimulation) cancelAnimationFrame(graphSimulation);

  function tick() {
    const query = searchInput ? searchInput.value.toLowerCase().trim() : '';
    const repulsion = sliderRep ? parseFloat(sliderRep.value) : 950;
    const baseLinkDist = sliderDist ? parseFloat(sliderDist.value) : 110;
    const nodeBaseRadius = sliderSize ? parseFloat(sliderSize.value) : 6;

    // Repulsion between pairs
    for (let i = 0; i < nodes.length; i++) {
      const a = nodes[i];
      if (a === dragNode) continue;
      for (let j = i + 1; j < nodes.length; j++) {
        const b = nodes[j];
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const d2 = dx * dx + dy * dy + 1;
        const force = repulsion / d2;
        const fx = (dx / Math.sqrt(d2)) * force;
        const fy = (dy / Math.sqrt(d2)) * force;
        a.vx -= fx * 0.005;
        a.vy -= fy * 0.005;
        b.vx += fx * 0.005;
        b.vy += fy * 0.005;
      }
    }

    // Edge attraction springs
    for (const edge of edges) {
      const a = nodeMap.get(edge.source);
      const b = nodeMap.get(edge.target);
      const dx = b.x - a.x;
      const dy = b.y - a.y;
      const dist = Math.sqrt(dx * dx + dy * dy) || 1;
      const targetDist = edge.similarity ? baseLinkDist * (1.2 - edge.similarity * 0.5) : baseLinkDist;
      const force = (dist - targetDist) * 0.004;

      const fx = (dx / dist) * force;
      const fy = (dy / dist) * force;
      if (a !== dragNode) { a.vx += fx; a.vy += fy; }
      if (b !== dragNode) { b.vx -= fx; b.vy -= fy; }
    }

    // Velocity update & damping
    for (const n of nodes) {
      if (n === dragNode) continue;
      n.vx *= 0.88;
      n.vy *= 0.88;
      n.x += n.vx;
      n.y += n.vy;

      // Keep within circular radius
      const r = Math.hypot(n.x, n.y);
      const maxR = Math.min(width, height) * 0.42;
      if (r > maxR) {
        n.x *= maxR / r;
        n.y *= maxR / r;
      }
    }

    // Render Canvas
    ctx.clearRect(0, 0, width, height);
    ctx.save();
    ctx.translate(offsetX, offsetY);
    ctx.scale(scale, scale);

    // Draw Edges
    for (const edge of edges) {
      const a = nodeMap.get(edge.source);
      const b = nodeMap.get(edge.target);

      const isConnectedToHovered = hoveredNode && (hoveredNode === a || hoveredNode === b);
      const isHighlightedPair = (a.id === 'keerthan-test-1' && b.id === 'chapter-1') || (a.id === 'chapter-1' && b.id === 'keerthan-test-1');

      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(b.x, b.y);

      if (isHighlightedPair || isConnectedToHovered) {
        ctx.strokeStyle = '#c792ea';
        ctx.lineWidth = 2.2;
        ctx.shadowColor = '#c792ea';
        ctx.shadowBlur = 8;
      } else if (edge.similarity !== null) {
        ctx.strokeStyle = 'rgba(97, 197, 255, 0.35)';
        ctx.lineWidth = Math.max(0.8, edge.similarity * 2.2);
        ctx.shadowBlur = 0;
      } else {
        ctx.strokeStyle = 'rgba(199, 146, 234, 0.3)';
        ctx.lineWidth = 1.0;
        ctx.shadowBlur = 0;
      }
      ctx.stroke();
      ctx.shadowBlur = 0;
    }

    // Draw Nodes
    const showLabels = chkLabels ? chkLabels.checked : true;

    for (const n of nodes) {
      const matchesSearch = !query || n.title.toLowerCase().includes(query);
      const isHovered = hoveredNode === n;
      const isHighlightNode = n.id === 'chapter-1' || n.id === 'keerthan-test-1';

      const r = isHovered ? nodeBaseRadius * 1.5 : (isHighlightNode ? nodeBaseRadius * 1.25 : nodeBaseRadius);

      ctx.beginPath();
      ctx.arc(n.x, n.y, r, 0, Math.PI * 2);

      if (!matchesSearch) {
        ctx.fillStyle = 'rgba(120, 120, 120, 0.2)';
      } else if (isHovered || isHighlightNode) {
        ctx.fillStyle = isHighlightNode ? '#a855f7' : '#ffffff';
        ctx.shadowColor = '#c792ea';
        ctx.shadowBlur = 14;
      } else {
        ctx.fillStyle = n.source_type === 'web' ? '#61c5ff' : '#9ca3af';
        ctx.shadowBlur = 0;
      }
      ctx.fill();
      ctx.shadowBlur = 0;

      // Label
      if (showLabels && (scale > 0.65 || isHovered || isHighlightNode) && matchesSearch) {
        ctx.font = `${isHovered || isHighlightNode ? '600' : '400'} ${11 / Math.max(0.8, scale)}px Inter, sans-serif`;
        ctx.fillStyle = isHovered || isHighlightNode ? '#ffffff' : 'rgba(220, 220, 225, 0.7)';
        ctx.textAlign = 'center';
        ctx.fillText(n.title, n.x, n.y + r + 13);
      }
    }

    ctx.restore();
    graphSimulation = requestAnimationFrame(tick);
  }

  graphSimulation = requestAnimationFrame(tick);

  // Mouse / Canvas Events
  function getMouseGraphCoords(e) {
    const rect = canvas.getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    return {
      gx: (mx - offsetX) / scale,
      gy: (my - offsetY) / scale,
      screenX: mx,
      screenY: my
    };
  }

  function findNodeUnder(gx, gy) {
    for (let i = nodes.length - 1; i >= 0; i--) {
      const n = nodes[i];
      const dist = Math.hypot(n.x - gx, n.y - gy);
      if (dist <= n.radius + 6) return n;
    }
    return null;
  }

  canvas.onmousedown = (e) => {
    const { gx, gy, screenX, screenY } = getMouseGraphCoords(e);
    const hit = findNodeUnder(gx, gy);
    if (hit) {
      dragNode = hit;
    } else {
      isDragging = true;
      lastMouse = { x: screenX, y: screenY };
    }
  };

  canvas.onmousemove = (e) => {
    const { gx, gy, screenX, screenY } = getMouseGraphCoords(e);

    if (dragNode) {
      dragNode.x = gx;
      dragNode.y = gy;
      dragNode.vx = 0;
      dragNode.vy = 0;
      return;
    }

    if (isDragging) {
      offsetX += screenX - lastMouse.x;
      offsetY += screenY - lastMouse.y;
      lastMouse = { x: screenX, y: screenY };
      return;
    }

    // Hover
    const hit = findNodeUnder(gx, gy);
    if (hit !== hoveredNode) {
      hoveredNode = hit;
      if (hit && tooltip) {
        tooltip.classList.remove('hidden');
        document.getElementById('tooltip-title').textContent = hit.title;
        document.getElementById('tooltip-type').textContent = `Type: ${hit.source_type || 'note'}`;
        document.getElementById('tooltip-summary').textContent = hit.summary || hit.content_preview || 'No preview available.';
        
        const tagsRow = document.getElementById('tooltip-tags');
        tagsRow.innerHTML = '';
        (hit.tags || []).forEach(t => {
          const p = document.createElement('span');
          p.className = 'tag-pill';
          p.textContent = `#${t}`;
          tagsRow.appendChild(p);
        });

        tooltip.style.left = `${screenX + 16}px`;
        tooltip.style.top = `${screenY + 16}px`;
      } else if (tooltip) {
        tooltip.classList.add('hidden');
      }
    }
  };

  window.onmouseup = () => {
    isDragging = false;
    dragNode = null;
  };

  canvas.onclick = (e) => {
    const { gx, gy } = getMouseGraphCoords(e);
    const hit = findNodeUnder(gx, gy);
    if (hit) {
      openNote(hit.id);
    }
  };

  canvas.onwheel = (e) => {
    e.preventDefault();
    const factor = e.deltaY < 0 ? 1.12 : 0.89;
    const { screenX, screenY } = getMouseGraphCoords(e);

    const newScale = Math.max(0.2, Math.min(4.0, scale * factor));
    offsetX = screenX - (screenX - offsetX) * (newScale / scale);
    offsetY = screenY - (screenY - offsetY) * (newScale / scale);
    scale = newScale;
  };

  // Zoom / Reset Controls
  document.getElementById('btn-graph-zoom-in')?.addEventListener('click', () => {
    scale = Math.min(4.0, scale * 1.25);
  });
  document.getElementById('btn-graph-zoom-out')?.addEventListener('click', () => {
    scale = Math.max(0.2, scale * 0.8);
  });
  document.getElementById('btn-graph-reset')?.addEventListener('click', () => {
    scale = 1.0;
    offsetX = width / 2;
    offsetY = height / 2;
  });
  document.getElementById('btn-graph-settings')?.addEventListener('click', () => {
    document.getElementById('graph-settings-panel')?.classList.toggle('hidden');
  });
}

// -------------------------------------------------------------
// 7. RAINBOW BOOKMARK BUTTON ACTION (Pic 3)
// -------------------------------------------------------------
async function triggerBookmarkFile() {
  try {
    showToast('Opening file selector...');
    const result = await tauriInvoke('bookmark_file');
    if (result) {
      showToast(`Bookmarked: ${result.title}`);
      await loadVault();
      openNote(result.id);
    }
  } catch (err) {
    showToast(`Bookmark error: ${err}`);
  }
}

// -------------------------------------------------------------
// 8. GLOBAL HOTKEYS & EVENT WIRING
// -------------------------------------------------------------
function setupEventListeners() {
  // Rainbow Bookmark Button (Image 3)
  const rainbowBtn = document.getElementById('rainbow-bookmark-btn');
  if (rainbowBtn) {
    rainbowBtn.addEventListener('click', triggerBookmarkFile);
  }

  // Ribbon Buttons
  document.getElementById('ribbon-files-btn')?.addEventListener('click', () => {
    document.getElementById('obsidian-sidebar').classList.toggle('hidden');
  });
  document.getElementById('ribbon-search-btn')?.addEventListener('click', openSearchModal);
  document.getElementById('ribbon-graph-btn')?.addEventListener('click', showGraphView);
  document.getElementById('ribbon-bookmark-list-btn')?.addEventListener('click', () => {
    const input = document.getElementById('sidebar-filter-input');
    if (input) {
      input.value = 'web';
      renderSidebarTree('web');
      input.focus();
    }
  });
  document.getElementById('ribbon-screenshot-btn')?.addEventListener('click', async () => {
    showToast('Taking screenshot...');
    await tauriInvoke('capture_screenshot');
    showToast('Screenshot saved to vault.');
    loadVault();
  });
  document.getElementById('ribbon-screen-search-btn')?.addEventListener('click', async () => {
    showToast('Analyzing screen text (Ctrl+Shift+R)...');
    try {
      const results = await tauriInvoke('analyze_screen_text');
      showToast(`Found ${results ? results.length : 0} matching bookmarks`);
      openSearchModal();
      renderSearchResults(results);
    } catch (e) {
      showToast(`Screen search: ${e}`);
    }
  });

  document.getElementById('ribbon-shortcuts-btn')?.addEventListener('click', () => {
    document.getElementById('settings-modal').classList.remove('hidden');
  });
  document.getElementById('ribbon-settings-btn')?.addEventListener('click', () => {
    document.getElementById('settings-modal').classList.remove('hidden');
  });

  // Sidebar filter
  document.getElementById('sidebar-filter-input')?.addEventListener('input', (e) => {
    renderSidebarTree(e.target.value);
  });

  // New Note Button
  document.getElementById('btn-sidebar-new-note')?.addEventListener('click', () => {
    const id = `note-${Date.now().toString(36)}`;
    const newNote = { id, title: 'Untitled Note', folder: 'Quick Notes', content: '# Untitled Note\n\n' };
    state.notesById.set(id, newNote);
    openNote(id);
  });

  // Add Tab Button
  document.getElementById('btn-add-tab')?.addEventListener('click', () => {
    const id = `note-${Date.now().toString(36)}`;
    const newNote = { id, title: 'Untitled Note', folder: 'Quick Notes', content: '# Untitled Note\n\n' };
    state.notesById.set(id, newNote);
    openNote(id);
  });

  // Toggle Edit / Preview
  document.getElementById('btn-toggle-preview')?.addEventListener('click', () => {
    const readerEl = document.getElementById('note-reader-content');
    const editorEl = document.getElementById('note-editor-content');
    state.isEditMode = !state.isEditMode;

    if (state.isEditMode) {
      readerEl.classList.add('hidden');
      editorEl.classList.remove('hidden');
      editorEl.focus();
    } else {
      const note = state.notesById.get(state.currentTabId);
      if (note) {
        note.content = editorEl.value;
        readerEl.innerHTML = renderMarkdown(note.content);
        updateWordCounts(note.content);
        tauriInvoke('save_bookmark_detail', { bookmark: note });
      }
      readerEl.classList.remove('hidden');
      editorEl.classList.add('hidden');
    }
  });

  // Graph button in note header
  document.getElementById('btn-open-graph-for-note')?.addEventListener('click', showGraphView);

  // Settings: Register Windows Explorer Context Menu
  document.getElementById('btn-register-context-menu')?.addEventListener('click', async () => {
    const statusEl = document.getElementById('ctx-reg-status');
    try {
      const msg = await tauriInvoke('register_windows_context_menu');
      statusEl.textContent = msg || 'Windows context menu registered!';
      showToast('Windows Context Menu Registered successfully!');
    } catch (err) {
      statusEl.textContent = `Error: ${err}`;
    }
  });

  // Modals close
  document.getElementById('btn-close-search-modal')?.addEventListener('click', () => {
    document.getElementById('search-modal').classList.add('hidden');
  });
  document.getElementById('btn-close-settings-modal')?.addEventListener('click', () => {
    document.getElementById('settings-modal').classList.add('hidden');
  });

  // Search input in modal
  document.getElementById('modal-search-input')?.addEventListener('input', async (e) => {
    const q = e.target.value;
    try {
      const res = await tauriInvoke('search_bookmarks', { query: q });
      renderSearchResults(res);
    } catch (err) {
      console.warn(err);
    }
  });

  // Window Controls (Frameless Titlebar)
  document.getElementById('btn-win-min')?.addEventListener('click', () => {
    tauriInvoke('minimize_window');
  });
  document.getElementById('btn-win-max')?.addEventListener('click', () => {
    tauriInvoke('toggle_maximize_window');
  });
  document.getElementById('btn-win-close')?.addEventListener('click', () => {
    tauriInvoke('close_window');
  });

  // Keyboard Shortcuts inside Window
  window.addEventListener('keydown', (e) => {
    // Ctrl + Shift + O -> Bookmark File
    if (e.ctrlKey && e.shiftKey && (e.key === 'O' || e.key === 'o')) {
      e.preventDefault();
      triggerBookmarkFile();
    }
    // Ctrl + Shift + S -> Screenshot
    else if (e.ctrlKey && e.shiftKey && (e.key === 'S' || e.key === 's')) {
      e.preventDefault();
      tauriInvoke('capture_screenshot');
      showToast('Screenshot taken.');
    }
    // Ctrl + Shift + B -> Capture Selection
    else if (e.ctrlKey && e.shiftKey && (e.key === 'B' || e.key === 'b')) {
      e.preventDefault();
      tauriInvoke('capture_selection');
      showToast('Bookmark selection saved.');
    }
    // Ctrl + Shift + F -> Search Bookmarks
    else if (e.ctrlKey && e.shiftKey && (e.key === 'F' || e.key === 'f')) {
      e.preventDefault();
      openSearchModal();
    }
    // Ctrl + Shift + R -> Screen Text Analysis
    else if (e.ctrlKey && e.shiftKey && (e.key === 'R' || e.key === 'r')) {
      e.preventDefault();
      showToast('Analyzing screen text (Ctrl+Shift+R)...');
      tauriInvoke('analyze_screen_text').then(results => {
        openSearchModal();
        renderSearchResults(results);
      });
    }
    // Ctrl + Shift + G -> Graph View
    else if (e.ctrlKey && e.shiftKey && (e.key === 'G' || e.key === 'g')) {
      e.preventDefault();
      showGraphView();
    }
    // Escape closes modals
    else if (e.key === 'Escape') {
      document.getElementById('search-modal').classList.add('hidden');
      document.getElementById('settings-modal').classList.add('hidden');
    }
  });

  // Listen to Tauri backend events if Tauri event emitter is available
  if (window.__TAURI__ && window.__TAURI__.event) {
    window.__TAURI__.event.listen('hotkey-bookmark-file', triggerBookmarkFile);
    window.__TAURI__.event.listen('hotkey-search', openSearchModal);
    window.__TAURI__.event.listen('hotkey-screen-search', () => {
      showToast('Screen search triggered.');
      openSearchModal();
    });
    window.__TAURI__.event.listen('open-graph-view', showGraphView);
    window.__TAURI__.event.listen('tray-bookmark-file', triggerBookmarkFile);
    window.__TAURI__.event.listen('tray-screen-search', openSearchModal);
  }
}

function openSearchModal() {
  const modal = document.getElementById('search-modal');
  modal.classList.remove('hidden');
  const input = document.getElementById('modal-search-input');
  input.value = '';
  input.focus();
  // Show all initially
  tauriInvoke('search_bookmarks', { query: '' }).then(renderSearchResults);
}

function renderSearchResults(items = []) {
  const container = document.getElementById('modal-search-results');
  container.innerHTML = '';
  if (!items || items.length === 0) {
    container.innerHTML = '<div style="padding: 16px; color: var(--text-muted); font-size: 13px;">No bookmarks found.</div>';
    return;
  }

  items.slice(0, 30).forEach(item => {
    const el = document.createElement('div');
    el.className = 'search-result-item';
    el.innerHTML = `
      <div class="result-title">${escapeHtml(item.title)}</div>
      <div class="result-snippet">${escapeHtml(item.summary || item.content_preview || '')}</div>
    `;
    el.addEventListener('click', () => {
      document.getElementById('search-modal').classList.add('hidden');
      openNote(item.id);
    });
    container.appendChild(el);
  });
}

function showToast(message) {
  const toast = document.getElementById('toast');
  toast.textContent = message;
  toast.classList.remove('hidden');
  setTimeout(() => {
    toast.classList.add('hidden');
  }, 3200);
}

// -------------------------------------------------------------
// 9. UTILITIES (Markdown & Sanitization)
// -------------------------------------------------------------
function renderMarkdown(md = '') {
  if (!md) return '';
  return md
    .split('\n\n')
    .map(p => {
      const trimmed = p.trim();
      if (trimmed.startsWith('# ')) return `<h1>${escapeHtml(trimmed.slice(2))}</h1>`;
      if (trimmed.startsWith('## ')) return `<h2>${escapeHtml(trimmed.slice(3))}</h2>`;
      if (trimmed.startsWith('### ')) return `<h3>${escapeHtml(trimmed.slice(4))}</h3>`;
      if (trimmed.startsWith('> ')) return `<blockquote>${escapeHtml(trimmed.slice(2))}</blockquote>`;
      // Replace [[Note]] wiki links
      const formatted = escapeHtml(trimmed).replace(/\[\[(.*?)\]\]/g, '<span class="tag-pill" style="cursor:pointer;" onclick="openNote(\'$1\')">[[$1]]</span>');
      return `<p>${formatted}</p>`;
    })
    .join('');
}

function escapeHtml(str = '') {
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

// -------------------------------------------------------------
// INITIALIZATION
// -------------------------------------------------------------
window.addEventListener('DOMContentLoaded', async () => {
  initUniverseAnimation();
  setupEventListeners();
  await loadVault();

  // Open default note "draft" as shown in Image 1
  openNote('note-draft');
});
