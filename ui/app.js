/**
 * Moneta - Obsidian-Style Local Knowledge Vault
 * 100% Offline • No AI APIs Used
 */

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
// APP STATE (Connected to real local vault bookmarks)
// -------------------------------------------------------------
let state = {
  currentTabId: 'graph-view',
  openTabs: [
    { id: 'graph-view', title: 'Graph view', isGraph: true }
  ],
  vaultItems: [],
  folders: ['Web Bookmarks', 'Images & OCR', 'Documents', 'Quick Notes'],
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

  // The intro always plays fully and then exits automatically.
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
  if (cmd === 'take_pending_shortcut_actions') {
    return [];
  }
  if (cmd === 'get_vault_structure') {
    const bms = [];
    return {
      folders: ['Writing', 'Web Bookmarks', 'Images & OCR', 'Documents', 'Quick Notes'],
      bookmarks: bms
    };
  }
  if (cmd === 'get_graph_data') {
    return { nodes: [], edges: [], search_history: [], search_history_status: 'ok' };
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
  state.notesById.clear();

  try {
    const res = await tauriInvoke('get_vault_structure');
    if (res) {
      if (res.folders && Array.isArray(res.folders)) {
        state.folders = res.folders.filter(f => f !== 'Writing');
      }
      if (res.bookmarks && Array.isArray(res.bookmarks)) {
        res.bookmarks.forEach(bm => {
          const folder = (bm.folder === 'Writing' || !bm.folder) ? 'Web Bookmarks' : bm.folder;
          state.notesById.set(bm.id, {
            id: bm.id,
            title: bm.title || bm.id,
            folder: folder,
            content: bm.content_preview || bm.summary || '',
            source_url: bm.source_url,
            tags: bm.tags || [],
            created_at: bm.created_at,
            source_type: bm.source_type
          });
        });
      }
    }

    // Open first real bookmark in tabs if only graph-view was open
    if (state.notesById.size > 0 && state.openTabs.length === 1 && state.openTabs[0].id === 'graph-view') {
      const firstNote = Array.from(state.notesById.values())[0];
      state.openTabs.unshift({ id: firstNote.id, title: firstNote.title });
      state.currentTabId = firstNote.id;
    }
  } catch (err) {
    console.warn('Vault load error:', err);
  }

  renderTabs();
  renderSidebarTree();
}

function renderSidebarTree(filterText = '') {
  const container = document.getElementById('vault-tree');
  if (!container) return;

  const filter = filterText.toLowerCase().trim();
  const allNotes = Array.from(state.notesById.values());

  if (allNotes.length === 0) {
    container.innerHTML = `
      <div style="padding: 24px 16px; text-align: center; color: var(--text-muted); font-size: 12.5px;">
        <p style="margin-bottom: 8px;">No bookmarks found.</p>
        <p style="font-size: 11px; opacity: 0.8;">Click <strong>Bookmark file</strong> above or press <kbd style="background: rgba(255,255,255,0.1); padding: 2px 5px; border-radius: 4px;">Ctrl+Shift+O</kbd> to add files.</p>
      </div>
    `;
    return;
  }

  // Group notes into folders (never include 'Writing')
  const groups = new Map();
  const validFolders = (state.folders || ['Web Bookmarks', 'Images & OCR', 'Documents', 'Quick Notes']).filter(f => f !== 'Writing');
  validFolders.forEach(f => groups.set(f, []));

  for (const note of allNotes) {
    if (filter) {
      const matchTitle = (note.title || '').toLowerCase().includes(filter);
      const matchTags = (note.tags || []).some(t => t.toLowerCase().includes(filter));
      if (!matchTitle && !matchTags) continue;
    }

    let fName = groups.has(note.folder) ? note.folder : 'Web Bookmarks';
    if (!groups.has(fName)) groups.set(fName, []);
    groups.get(fName).push(note);
  }

  container.innerHTML = '';

  for (const [folderName, items] of groups.entries()) {
    if (items.length === 0) continue; // Only show non-empty folders
    if (filter && items.length === 0) continue;

    const folderEl = document.createElement('div');
    folderEl.className = 'tree-folder open';

    const headerEl = document.createElement('div');
    headerEl.className = 'tree-folder-header';
    headerEl.innerHTML = `
      <span class="folder-arrow">&#9658;</span>
      <span class="folder-name">${escapeHtml(folderName)}</span>
      <span class="folder-count">${items.length}</span>
      <button class="tree-folder-delete" title="Delete Folder" data-folder="${escapeHtml(folderName)}">🗑</button>
    `;

    headerEl.addEventListener('click', (e) => {
      if (e.target.closest('.tree-folder-delete')) {
        e.stopPropagation();
        const fName = e.target.closest('.tree-folder-delete').dataset.folder;
        if (confirm(`Delete folder "${fName}" and all ${items.length} items inside?`)) {
          tauriInvoke('delete_folder', { folder: fName }).then(() => {
            items.forEach(it => state.notesById.delete(it.id));
            state.openTabs = state.openTabs.filter(t => !items.some(it => it.id === t.id));
            showToast(`Folder "${fName}" deleted.`);
            renderTabs();
            renderSidebarTree();
          }).catch(err => showToast(`Delete failed: ${err}`));
        }
        return;
      }
      folderEl.classList.toggle('open');
    });

    const childrenEl = document.createElement('div');
    childrenEl.className = 'tree-folder-children';

    items.forEach(note => {
      const itemEl = document.createElement('div');
      itemEl.className = `tree-item ${state.currentTabId === note.id ? 'active' : ''}`;
      itemEl.dataset.noteId = note.id;

      let badgeHtml = '';
      if (note.source_type && note.source_type !== 'note') {
        badgeHtml = `<span class="tree-item-badge">${escapeHtml(note.source_type)}</span>`;
      }

      itemEl.innerHTML = `
        <span class="tree-item-title">${escapeHtml(note.title)}</span>
        ${badgeHtml}
        <button class="tree-item-delete" title="Delete file" data-id="${escapeHtml(note.id)}">&times;</button>
      `;

      itemEl.addEventListener('click', (e) => {
        if (e.target.closest('.tree-item-delete')) {
          e.stopPropagation();
          const nId = e.target.closest('.tree-item-delete').dataset.id;
          if (confirm(`Delete bookmark "${note.title}"?`)) {
            tauriInvoke('delete_bookmark', { id: nId }).then(() => {
              state.notesById.delete(nId);
              closeTab(nId);
              showToast(`Bookmark deleted.`);
              renderSidebarTree();
            }).catch(err => showToast(`Delete failed: ${err}`));
          }
          return;
        }
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
async function openNote(noteId) {
  let note = state.notesById.get(noteId);
  if (!note) {
    note = { id: noteId, title: noteId, content: `# ${noteId}\n\n`, folder: 'Documents' };
    state.notesById.set(noteId, note);
  }

  // Fetch full details & OCR text from backend
  try {
    const detail = await tauriInvoke('get_bookmark_detail', { id: noteId });
    if (detail) {
      note = {
        ...note,
        ...detail,
        content: detail.captured_text || detail.ocr_text || note.content || '',
        ocr_text: detail.ocr_text,
        path: detail.path
      };
      state.notesById.set(noteId, note);
    }
  } catch (e) {
    showToast(`Could not load bookmark details: ${e}`);
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

    dateEl.textContent = note.created_at ? new Date(note.created_at * 1000).toLocaleString() : 'Captured';
  } else {
    metaCard.classList.add('hidden');
  }

  // Content
  const readerEl = document.getElementById('note-reader-content');
  const editorEl = document.getElementById('note-editor-content');

  readerEl.innerHTML = formatBookmarkView(note);
  editorEl.value = note.content || '';

  updateWordCounts(note.content || '');
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
    state.openTabs.push({ id: 'graph-view', title: 'Graph view', isGraph: true });
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

  if (!rawData || !Array.isArray(rawData.nodes)) {
    rawData = mockBackend('get_graph_data');
  }

  // Populate graph nodes with force physics variables spread across wide area
  const nodeCount = rawData.nodes.length;
  const nodes = rawData.nodes.map((n, i) => {
    const angle = i * 2.399;
    const r = Math.min(width, height) * 0.75 * Math.sqrt((i + 1) / nodeCount);
    return {
      ...n,
      x: Math.cos(angle) * r,
      y: Math.sin(angle) * r,
      vx: 0,
      vy: 0,
      radius: 3.5
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

  let scale = 0.95;
  let offsetX = width / 2;
  let offsetY = height / 2;
  let isDragging = false;
  let dragNode = null;
  let hoveredNode = null;
  let selectedNode = null;
  let lastMouse = { x: 0, y: 0 };
  let mouseDownPosition = null;
  let pressedNode = null;
  let dragMoved = false;
  let lastGraphActivation = { id: null, surface: null, time: 0, x: null, y: null };
  const miniPanel = document.getElementById('graph-mini-panel');
  const miniCanvas = document.getElementById('graph-mini-canvas');
  const miniTitle = document.getElementById('graph-mini-title');
  const miniCount = document.getElementById('graph-mini-count');
  let miniHitTargets = [];

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
    const repulsion = sliderRep ? parseFloat(sliderRep.value) : 3200;
    const baseLinkDist = sliderDist ? parseFloat(sliderDist.value) : 190;
    const nodeBaseRadius = sliderSize ? parseFloat(sliderSize.value) : 3.5;

    // Repulsion between pairs (strong Obsidian spread)
    for (let i = 0; i < nodes.length; i++) {
      const a = nodes[i];
      if (a === dragNode) continue;
      for (let j = i + 1; j < nodes.length; j++) {
        const b = nodes[j];
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const d2 = dx * dx + dy * dy + 60;
        const force = repulsion / d2;
        const fx = (dx / Math.sqrt(d2)) * force;
        const fy = (dy / Math.sqrt(d2)) * force;
        a.vx -= fx * 0.0035;
        a.vy -= fy * 0.0035;
        b.vx += fx * 0.0035;
        b.vy += fy * 0.0035;
      }
    }

    // Edge attraction springs
    for (const edge of edges) {
      const a = nodeMap.get(edge.source);
      const b = nodeMap.get(edge.target);
      const dx = b.x - a.x;
      const dy = b.y - a.y;
      const dist = Math.sqrt(dx * dx + dy * dy) || 1;
      const targetDist = edge.similarity ? baseLinkDist * (1.3 - edge.similarity * 0.6) : baseLinkDist;
      const force = (dist - targetDist) * 0.0025;

      const fx = (dx / dist) * force;
      const fy = (dy / dist) * force;
      if (a !== dragNode) { a.vx += fx; a.vy += fy; }
      if (b !== dragNode) { b.vx -= fx; b.vy -= fy; }
    }

    // Velocity update & damping (natural Obsidian feel, no boundary clamping)
    for (const n of nodes) {
      if (n === dragNode) continue;
      n.vx *= 0.85;
      n.vy *= 0.85;
      n.x += n.vx;
      n.y += n.vy;
    }

    // Render Canvas
    ctx.clearRect(0, 0, width, height);
    ctx.save();
    ctx.translate(offsetX, offsetY);
    ctx.scale(scale, scale);

    // Draw Edges (subtle hairline)
    for (const edge of edges) {
      const a = nodeMap.get(edge.source);
      const b = nodeMap.get(edge.target);

      const isConnectedToHovered = hoveredNode && (hoveredNode === a || hoveredNode === b);

      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(b.x, b.y);

      if (isConnectedToHovered) {
        ctx.strokeStyle = 'rgba(192, 132, 252, 0.75)';
        ctx.lineWidth = 1.4;
      } else if (edge.similarity !== null) {
        ctx.strokeStyle = 'rgba(125, 207, 255, 0.2)';
        ctx.lineWidth = 0.6;
      } else {
        ctx.strokeStyle = 'rgba(148, 163, 184, 0.12)';
        ctx.lineWidth = 0.5;
      }
      ctx.stroke();
    }

    // Draw Nodes (Obsidian-style glowing stars)
    const showLabels = chkLabels ? chkLabels.checked : true;

    for (const n of nodes) {
      const matchesSearch = !query || n.title.toLowerCase().includes(query);
      const isHovered = hoveredNode === n;
      const isSelected = selectedNode === n;
      const isNeighbor = hoveredNode && adj.get(hoveredNode.id)?.some(item => item.target === n);

      const r = isHovered ? nodeBaseRadius * 1.6 : (isSelected ? nodeBaseRadius * 1.5 : (isNeighbor ? nodeBaseRadius * 1.25 : nodeBaseRadius));

      ctx.beginPath();
      ctx.arc(n.x, n.y, r, 0, Math.PI * 2);

      if (!matchesSearch) {
        ctx.fillStyle = 'rgba(80, 85, 100, 0.2)';
      } else if (isHovered || isSelected) {
        ctx.fillStyle = '#ffffff';
        ctx.shadowColor = '#c084fc';
        ctx.shadowBlur = 10;
      } else if (isNeighbor) {
        ctx.fillStyle = '#c084fc';
        ctx.shadowColor = '#c084fc';
        ctx.shadowBlur = 6;
      } else {
        ctx.fillStyle = n.source_type === 'web' ? '#7aa2f7' : (n.source_type === 'image' || n.source_type === 'screenshot') ? '#bb9af7' : '#34d399';
        ctx.shadowBlur = 0;
      }
      ctx.fill();
      ctx.shadowBlur = 0;

      // Draw Labels ONLY on hover, direct neighbors, or high zoom level
      if (showLabels && matchesSearch && (isHovered || isSelected || isNeighbor || scale > 1.35)) {
        ctx.font = `400 ${10 / Math.max(0.8, scale)}px Inter, sans-serif`;
        ctx.fillStyle = isHovered ? '#ffffff' : 'rgba(203, 213, 225, 0.85)';
        ctx.textAlign = 'center';
        const displayTitle = n.title.length > 22 ? `${n.title.slice(0, 20)}...` : n.title;
        ctx.fillText(displayTitle, n.x, n.y + r + 10);
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

  function selectGraphNode(node) {
    selectedNode = node;
    if (!node || !miniPanel || !miniCanvas) return;

    const connections = adj.get(node.id) || [];
    const dpr = window.devicePixelRatio || 1;
    const miniWidth = miniCanvas.clientWidth || 326;
    const miniHeight = miniCanvas.clientHeight || 190;
    miniCanvas.width = Math.round(miniWidth * dpr);
    miniCanvas.height = Math.round(miniHeight * dpr);
    const miniCtx = miniCanvas.getContext('2d');
    miniCtx.setTransform(dpr, 0, 0, dpr, 0, 0);
    miniCtx.clearRect(0, 0, miniWidth, miniHeight);

    const center = { x: miniWidth / 2, y: miniHeight / 2 };
    const orbit = Math.max(30, Math.min(miniWidth * 0.39, miniHeight * 0.34));
    miniHitTargets = [{ node, x: center.x, y: center.y, radius: 8 }];
    const neighborPositions = connections.map((item, index) => {
      const angle = (2 * Math.PI * index / Math.max(1, connections.length)) - Math.PI / 2;
      return {
        node: item.target,
        x: center.x + Math.cos(angle) * orbit,
        y: center.y + Math.sin(angle) * orbit,
        radius: 6
      };
    });
    miniHitTargets.push(...neighborPositions);

    for (const item of connections) {
      const target = neighborPositions.find(position => position.node.id === item.target.id);
      if (!target) continue;
      miniCtx.beginPath();
      miniCtx.moveTo(center.x, center.y);
      miniCtx.lineTo(target.x, target.y);
      miniCtx.strokeStyle = item.edge.similarity !== null
        ? 'rgba(125, 207, 255, 0.55)'
        : 'rgba(192, 132, 252, 0.55)';
      miniCtx.lineWidth = 1;
      miniCtx.stroke();
    }

    for (const target of miniHitTargets) {
      miniCtx.beginPath();
      miniCtx.arc(target.x, target.y, target.radius, 0, Math.PI * 2);
      miniCtx.fillStyle = target.node.id === node.id
        ? '#ffffff'
        : target.node.source_type === 'web' ? '#7aa2f7' : '#bb9af7';
      miniCtx.shadowColor = target.node.id === node.id ? '#c084fc' : 'transparent';
      miniCtx.shadowBlur = target.node.id === node.id ? 8 : 0;
      miniCtx.fill();
      miniCtx.shadowBlur = 0;
      const label = target.node.title.length > 20
        ? `${target.node.title.slice(0, 18)}...`
        : target.node.title;
      miniCtx.font = '10px Inter, sans-serif';
      miniCtx.fillStyle = '#d1d5db';
      miniCtx.textAlign = 'center';
      miniCtx.fillText(label, target.x, target.y + target.radius + 12);
    }

    miniTitle.textContent = node.title;
    miniCount.textContent = connections.length === 1
      ? '1 direct connection'
      : `${connections.length} direct connections`;
    miniPanel.classList.remove('hidden');
  }

  function activateGraphNode(node, surface, position = null) {
    const now = performance.now();
    if (
      lastGraphActivation.id === node.id &&
      lastGraphActivation.surface === surface &&
      now - lastGraphActivation.time <= 500
    ) {
      lastGraphActivation = { id: null, surface: null, time: 0, x: null, y: null };
      openNote(node.id);
      return;
    }

    lastGraphActivation = {
      id: node.id,
      surface,
      time: now,
      x: position?.x ?? null,
      y: position?.y ?? null
    };
    selectGraphNode(node);
  }

  canvas.onmousedown = (e) => {
    const { gx, gy, screenX, screenY } = getMouseGraphCoords(e);
    mouseDownPosition = { x: screenX, y: screenY };
    dragMoved = false;
    const hit = findNodeUnder(gx, gy);
    pressedNode = hit;
    if (hit) {
      dragNode = hit;
    } else {
      isDragging = true;
      lastMouse = { x: screenX, y: screenY };
    }
  };

  canvas.onmousemove = (e) => {
    const { gx, gy, screenX, screenY } = getMouseGraphCoords(e);

    if (mouseDownPosition && Math.hypot(screenX - mouseDownPosition.x, screenY - mouseDownPosition.y) > 4) {
      dragMoved = true;
    }

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
    mouseDownPosition = null;
  };

  canvas.onclick = (e) => {
    if (dragMoved) {
      dragMoved = false;
      pressedNode = null;
      return;
    }
    const { gx, gy } = getMouseGraphCoords(e);
    const hit = pressedNode || findNodeUnder(gx, gy);
    pressedNode = null;
    if (hit) activateGraphNode(hit, 'main');
  };

  miniCanvas?.addEventListener('click', (e) => {
    const rect = miniCanvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    const now = performance.now();
    if (
      lastGraphActivation.surface === 'mini' &&
      now - lastGraphActivation.time <= 500 &&
      lastGraphActivation.x !== null &&
      Math.hypot(lastGraphActivation.x - x, lastGraphActivation.y - y) <= 20
    ) {
      const noteId = lastGraphActivation.id;
      lastGraphActivation = { id: null, surface: null, time: 0, x: null, y: null };
      openNote(noteId);
      return;
    }
    const hit = [...miniHitTargets].reverse().find(target =>
      Math.hypot(target.x - x, target.y - y) <= target.radius + 7
    );
    if (hit) activateGraphNode(hit.node, 'mini', { x, y });
  });
  document.getElementById('btn-graph-mini-close')?.addEventListener('click', () => {
    miniPanel?.classList.add('hidden');
    selectedNode = null;
  });

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

async function triggerCaptureSelection() {
  try {
    showToast('Capturing selected text...');
    const bm = await tauriInvoke('capture_selection');
    if (!bm) {
      showToast('No selected text was captured.');
      return;
    }

    await loadVault();
    openNote(bm.id);
    showToast('Selection bookmarked!');
  } catch (err) {
    showToast(`Selection: ${err || 'Failed'}`);
  }
}

const lastShortcutTime = new Map();

function runShortcutOnce(key) {
  const now = Date.now();
  if (now - (lastShortcutTime.get(key) || 0) < 500) return;
  lastShortcutTime.set(key, now);

  if (key === 'o') {
    triggerBookmarkFile();
  } else if (key === 's') {
    showToast('📸 Select screen area for screenshot...');
    tauriInvoke('capture_screenshot').then(async bm => {
      await loadVault();
      showToast('📸 Screenshot captured and bookmarked!');
      if (bm && bm.id) openNote(bm.id);
    }).catch(err => {
      showToast(`Screenshot: ${err || 'Cancelled'}`);
    });
  } else if (key === 'b') {
    triggerCaptureSelection();
  } else if (key === 'f') {
    openSearchModal();
  } else if (key === 'r') {
    showToast('🔍 Reading the full screen for context...');
    tauriInvoke('analyze_screen_text').then(data => {
      if (data) openSearchModalWithQuery(data.query, data.results);
    }).catch(err => {
      showToast(`Screen Analysis: ${err || 'No readable text detected'}`);
    });
  } else if (key === 'g') {
    showGraphView();
  }
}

async function openChromeSyncModal() {
  const modal = document.getElementById('chrome-sync-modal');
  const profileSelect = document.getElementById('chrome-profile-select');
  const status = document.getElementById('chrome-sync-status');
  const syncButton = document.getElementById('btn-confirm-chrome-sync');
  status.textContent = 'Checking Chrome profiles...';
  profileSelect.replaceChildren();
  syncButton.disabled = true;
  modal.classList.remove('hidden');

  try {
    const profiles = await tauriInvoke('get_chrome_profiles');
    if (!Array.isArray(profiles) || profiles.length === 0) {
      status.textContent = 'No readable Chrome profiles were found on this computer.';
      return;
    }
    for (const profile of profiles) {
      const option = document.createElement('option');
      option.value = profile.id;
      option.textContent = `${profile.name} (${profile.bookmark_count} bookmarks)`;
      profileSelect.appendChild(option);
    }
    status.textContent = `${profiles.length} Chrome profile${profiles.length === 1 ? '' : 's'} found.`;
    syncButton.disabled = false;
  } catch (error) {
    status.textContent = `Could not list Chrome profiles: ${error}`;
  }
}

async function syncSelectedChromeProfile() {
  const status = document.getElementById('chrome-sync-status');
  const syncButton = document.getElementById('btn-confirm-chrome-sync');
  const profileId = document.getElementById('chrome-profile-select').value;
  if (!profileId) {
    status.textContent = 'Select a Chrome profile to sync.';
    return;
  }

  syncButton.disabled = true;
  syncButton.textContent = 'Syncing...';
  status.textContent = 'Reading bookmarks and syncing them to your local vault...';
  try {
    const result = await tauriInvoke('sync_chrome_bookmarks', { profileId });
    await loadVault();
    if (document.getElementById('view-graph')?.classList.contains('active')) {
      await loadAndRenderGraph();
    }
    const failureText = result.page_content_failures
      ? ` ${result.page_content_failures} page(s) could not be enriched with webpage text.`
      : '';
    status.textContent = `Synced ${result.synced} bookmarks.${failureText}`;
    showToast(`Chrome sync complete: ${result.synced} bookmarks.`);
  } catch (error) {
    status.textContent = `Chrome sync failed: ${error}`;
  } finally {
    syncButton.disabled = false;
    syncButton.textContent = 'Sync profile';
  }
}

// -------------------------------------------------------------
// 8. GLOBAL HOTKEYS & EVENT WIRING
// -------------------------------------------------------------
async function setupEventListeners() {
  // Rainbow Bookmark Button (Image 3) & Ribbon Bookmark Button
  const rainbowBtn = document.getElementById('rainbow-bookmark-btn');
  if (rainbowBtn) {
    rainbowBtn.addEventListener('click', triggerBookmarkFile);
  }
  document.getElementById('ribbon-bookmark-file-btn')?.addEventListener('click', triggerBookmarkFile);
  document.getElementById('btn-sync-chrome-bookmarks')?.addEventListener('click', openChromeSyncModal);
  document.getElementById('btn-confirm-chrome-sync')?.addEventListener('click', syncSelectedChromeProfile);
  const closeChromeSyncModal = () => {
    document.getElementById('chrome-sync-modal')?.classList.add('hidden');
  };
  document.getElementById('btn-close-chrome-sync-modal')?.addEventListener('click', closeChromeSyncModal);
  document.getElementById('btn-cancel-chrome-sync')?.addEventListener('click', closeChromeSyncModal);

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
  // Delete Current Note Button
  document.getElementById('btn-delete-current-note')?.addEventListener('click', async () => {
    const noteId = state.currentTabId;
    const note = state.notesById.get(noteId);
    if (!note) return;
    if (confirm(`Delete bookmark "${note.title}"?`)) {
      try {
        await tauriInvoke('delete_bookmark', { id: noteId });
        state.notesById.delete(noteId);
        closeTab(noteId);
        showToast('Bookmark deleted.');
        renderSidebarTree();
      } catch (err) {
        showToast(`Delete failed: ${err}`);
      }
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
    const shortcutKey = e.key.toLowerCase();
    if (e.ctrlKey && e.shiftKey && ['o', 's', 'b', 'f', 'r', 'g'].includes(shortcutKey)) {
      e.preventDefault();
      runShortcutOnce(shortcutKey);
    }
    // Escape closes modals
    else if (e.key === 'Escape') {
      document.getElementById('search-modal').classList.add('hidden');
      document.getElementById('settings-modal').classList.add('hidden');
      document.getElementById('chrome-sync-modal').classList.add('hidden');
    }
  });

  // Listen to Tauri backend events if Tauri event emitter is available
  if (window.__TAURI__ && window.__TAURI__.event) {
    const shortcutListeners = [
      window.__TAURI__.event.listen('hotkey-bookmark-file', () => runShortcutOnce('o')),
      window.__TAURI__.event.listen('hotkey-capture-selection', () => runShortcutOnce('b')),
      window.__TAURI__.event.listen('hotkey-capture-screenshot', () => runShortcutOnce('s')),
      window.__TAURI__.event.listen('hotkey-search', () => runShortcutOnce('f')),
      window.__TAURI__.event.listen('hotkey-analyze-screen', () => runShortcutOnce('r')),
      window.__TAURI__.event.listen('open-graph-view', () => runShortcutOnce('g')),
    ];
    window.__TAURI__.event.listen('tray-bookmark-file', triggerBookmarkFile);

    // Screenshot events (Ctrl+Shift+S or System Tray)
    window.__TAURI__.event.listen('screenshot-in-progress', () => {
      showToast('📸 Select screen area for screenshot...');
    });
    window.__TAURI__.event.listen('screenshot-captured', async (event) => {
      const bm = event.payload;
      await loadVault();
      showToast('📸 Screenshot captured and bookmarked!');
      if (bm && bm.id) {
        openNote(bm.id);
      }
    });
    window.__TAURI__.event.listen('screenshot-error', (event) => {
      showToast(`Screenshot: ${event.payload || 'Cancelled'}`);
    });

    // Screen Analyze events (Ctrl+Shift+R or System Tray)
    window.__TAURI__.event.listen('screen-search-in-progress', () => {
      showToast('🔍 Reading the full screen for context...');
    });
    window.__TAURI__.event.listen('screen-search-results', (event) => {
      const data = event.payload;
      if (data) {
        openSearchModalWithQuery(data.query, data.results);
      }
    });
    window.__TAURI__.event.listen('screen-search-error', (event) => {
      showToast(`Screen Analysis: ${event.payload || 'No readable text detected'}`);
    });
    await Promise.all(shortcutListeners);
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

function openSearchModalWithQuery(query, results = []) {
  const modal = document.getElementById('search-modal');
  modal.classList.remove('hidden');
  const input = document.getElementById('modal-search-input');
  input.value = query || '';
  input.focus();
  renderSearchResults(results, query);
  showToast(`Analyzed screen: "${safeTruncateStr(query, 35)}"`);
}

function safeTruncateStr(str = '', max = 50) {
  if (!str) return '';
  return str.length > max ? str.slice(0, max) + '...' : str;
}

function renderSearchResults(items = [], activeQuery = '') {
  const container = document.getElementById('modal-search-results');
  container.innerHTML = '';

  if (activeQuery) {
    const banner = document.createElement('div');
    banner.style.cssText = 'padding: 10px 16px; font-size: 12px; color: var(--text-accent); background: rgba(124, 77, 255, 0.08); border-bottom: 1px solid var(--border-subtle);';
    banner.innerHTML = `🔍 Analyzed Screen Text (${items.length} match${items.length === 1 ? '' : 'es'}): <em>"${escapeHtml(safeTruncateStr(activeQuery, 50))}"</em>`;
    container.appendChild(banner);
  }

  if (!items || items.length === 0) {
    if (activeQuery) {
      const emptyBox = document.createElement('div');
      emptyBox.style.cssText = 'padding: 16px; color: var(--text-muted); font-size: 13px; line-height: 1.5;';
      emptyBox.innerHTML = `
        <div>No existing bookmarks matched the screen text:</div>
        <div style="margin-top: 8px; padding: 10px 12px; background: var(--bg-tertiary); border: 1px solid var(--border-subtle); border-radius: 6px; color: var(--text-normal); font-family: monospace; font-size: 12px; max-height: 140px; overflow-y: auto; white-space: pre-wrap;">${escapeHtml(activeQuery)}</div>
      `;
      container.appendChild(emptyBox);
    } else {
      container.innerHTML = '<div style="padding: 16px; color: var(--text-muted); font-size: 13px;">No bookmarks found.</div>';
    }
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
// 9. UTILITIES (Markdown, OCR formatting, & Sanitization)
// -------------------------------------------------------------
window.copyOcrText = function(noteId) {
  const note = state.notesById.get(noteId);
  if (!note || !note.ocr_text) return;
  navigator.clipboard.writeText(note.ocr_text).then(() => {
    showToast('OCR text copied to clipboard!');
  });
};

function formatBookmarkView(note) {
  let html = '';

  const isImage = note.source_type === 'image' || note.source_type === 'screenshot';
  if (isImage && note.path) {
    let imgSrc = '';
    if (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.convertFileSrc) {
      imgSrc = window.__TAURI__.core.convertFileSrc(note.path);
    } else {
      imgSrc = `file:///${note.path.replace(/\\/g, '/')}`;
    }

    html += `
      <div class="image-bookmark-preview">
        <img src="${imgSrc}" alt="${escapeHtml(note.title)}" onerror="this.hidden=true;this.nextElementSibling.hidden=false" />
        <p class="image-load-error" hidden>Could not load the captured image.</p>
      </div>
    `;

    const ocr = note.ocr_text || note.content || '';
    if (ocr) {
      html += `
        <div class="ocr-card">
          <div class="ocr-card-header">
            <span>Extracted OCR Text</span>
            <button class="ocr-copy-btn" onclick="copyOcrText('${escapeHtml(note.id)}')">Copy Text</button>
          </div>
          <div class="ocr-content-text">${escapeHtml(ocr)}</div>
        </div>
      `;
    }
  }

  const textContent = note.content || note.summary || '';
  if (textContent && !isImage) {
    html += renderMarkdown(textContent);
  } else if (!isImage) {
    html += '<p style="color: var(--text-faint); font-style: italic;">No text content available.</p>';
  }

  return html;
}

function renderMarkdown(md = '') {
  if (!md) return '';

  let clean = md.replace(/\r\n/g, '\n').replace(/\r/g, '\n');

  // Code blocks ```...```
  const codeBlocks = [];
  clean = clean.replace(/```([\s\S]*?)```/g, (match, code) => {
    const idx = codeBlocks.length;
    codeBlocks.push(`<pre><code>${escapeHtml(code.trim())}</code></pre>`);
    return `__CODEBLOCK_${idx}__`;
  });

  const paragraphs = clean.split(/\n{2,}/);
  return paragraphs.map(p => {
    let text = p.trim();
    if (!text) return '';

    if (text.startsWith('__CODEBLOCK_') && text.endsWith('__')) {
      const idx = parseInt(text.replace(/__CODEBLOCK_(\d+)__/, '$1'), 10);
      return codeBlocks[idx] || '';
    }

    if (text.startsWith('# ')) return `<h1>${formatInline(text.slice(2))}</h1>`;
    if (text.startsWith('## ')) return `<h2>${formatInline(text.slice(3))}</h2>`;
    if (text.startsWith('### ')) return `<h3>${formatInline(text.slice(4))}</h3>`;
    if (text.startsWith('#### ')) return `<h4>${formatInline(text.slice(5))}</h4>`;

    if (text.startsWith('> ')) {
      const quoteText = text.split('\n').map(l => l.replace(/^>\s?/, '')).join('<br>');
      return `<blockquote>${formatInline(quoteText)}</blockquote>`;
    }

    if (text.startsWith('- ') || text.startsWith('* ')) {
      const items = text.split('\n').map(line => {
        const itemContent = line.replace(/^[-*]\s+/, '');
        return `<li>${formatInline(itemContent)}</li>`;
      }).join('');
      return `<ul>${items}</ul>`;
    }

    if (/^\d+\.\s/.test(text)) {
      const items = text.split('\n').map(line => {
        const itemContent = line.replace(/^\d+\.\s+/, '');
        return `<li>${formatInline(itemContent)}</li>`;
      }).join('');
      return `<ol>${items}</ol>`;
    }

    if (text === '---' || text === '***') {
      return '<hr style="border: none; border-top: 1px solid var(--border-subtle); margin: 20px 0;">';
    }

    const formattedLines = text.split('\n').map(line => formatInline(line)).join('<br>');
    return `<p>${formattedLines}</p>`;
  }).join('');
}

function formatInline(str = '') {
  let s = escapeHtml(str);
  s = s.replace(/\*\*(.*?)\*\*/g, '<strong>$1</strong>');
  s = s.replace(/\*(.*?)\*/g, '<em>$1</em>');
  s = s.replace(/`([^`]+)`/g, '<code>$1</code>');
  s = s.replace(/\[\[(.*?)\]\]/g, '<span class="tag-pill" style="cursor:pointer;" onclick="openNote(\'$1\')">[[$1]]</span>');
  s = s.replace(/\[(.*?)\]\((https?:\/\/[^\s)]+)\)/g, '<a href="$2" target="_blank" class="meta-link">$1</a>');
  return s;
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
  await setupEventListeners();
  await loadVault();

  // Open first real bookmark if available, else show graph
  if (state.notesById.size > 0) {
    const firstId = Array.from(state.notesById.keys())[0];
    openNote(firstId);
  } else {
    showGraphView();
  }

  try {
    const pendingActions = await tauriInvoke('take_pending_shortcut_actions');
    if (Array.isArray(pendingActions)) {
      pendingActions.forEach((key) => runShortcutOnce(key));
    }
  } catch (error) {
    console.error('Failed to process startup shortcut:', error);
  }
});
