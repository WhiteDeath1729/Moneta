async function getAllBookmarks() {
    const tree = await chrome.bookmarks.getTree();

    const bookmarks = [];

    function walk(nodes, folderPath) {
        for (const node of nodes) {
            if (node.url) {
                bookmarks.push({
                    created_at_in_chrome: node.dateAdded,
                    title: node.title,
                    url: node.url,
                    folder_path: folderPath
                });
            }

            if (node.children) {
                const newFolderPath = node.url
                    ? folderPath
                    : folderPath
                        ? `${folderPath}/${node.title}`
                        : node.title;

                walk(node.children, newFolderPath);
            }
        }
    }

    walk(tree, "");

    return bookmarks;
}

async function sendBookmarksToMoneta() {
    let syncedCount = 0;
    const pageContentFailures = [];

    try {
        const bookmarks = await getAllBookmarks();

        console.log(`Found ${bookmarks.length} Chrome bookmarks`);

        const batchSize = 100;
        for (let offset = 0; offset < bookmarks.length; offset += batchSize) {
            const batch = bookmarks.slice(offset, offset + batchSize);
            const response = await fetch(
                "http://127.0.0.1:8765/bookmarks",
                {
                    method: "POST",
                    headers: {
                        "Content-Type": "application/json"
                    },
                    body: JSON.stringify(batch)
                }
            );

            const result = await response.text();
            console.log(
                "Moneta batch response:",
                response.status,
                result
            );

            if (!response.ok) {
                throw new Error(
                    `Moneta rejected a batch (HTTP ${response.status}): ${result}`
                );
            }

            const details = JSON.parse(result);
            syncedCount += details.synced;
            pageContentFailures.push(...details.page_content_failures);
        }

        return {
            success: true,
            count: syncedCount,
            message: pageContentFailures.length
                ? `Synced ${syncedCount}; page text could not be fetched for ${pageContentFailures.length} bookmarks.`
                : `Synced ${syncedCount} bookmarks with webpage text, tags, and embeddings.`
        };

    } catch (error) {
        console.error(
            "Failed to communicate with Moneta:",
            error
        );

        return {
            success: false,
            count: syncedCount,
            message: syncedCount > 0
                ? `Synced ${syncedCount} bookmarks before the sync failed: ${error.message}`
                : error.message
        };
    }
}

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (message.action === "syncBookmarks") {
        sendBookmarksToMoneta()
            .then(result => sendResponse(result));

        return true;
    }
});

chrome.runtime.onMessage.addListener(
    (message, sender, sendResponse) => {

        if (message.type !== "MONETA_BOOKMARK_SELECTION") {
            return;
        }

        console.log(
            "Sending selection to Moneta:",
            message.data
        );

        fetch("http://127.0.0.1:8765/selection", {
            method: "POST",

            headers: {
                "Content-Type": "application/json"
            },

            body: JSON.stringify({
                title: message.data.title,
                url: message.data.url,
                selected_text: message.data.selected_text
            })
        })
        .then(async response => {
            if (!response.ok) {
                throw new Error(
                    `Moneta returned HTTP ${response.status}`
                );
            }

            return response.json();
        })
        .then(data => {
            console.log(
                "Moneta saved bookmark:",
                data
            );
            if (data.page_content_error) {
                console.warn(
                    "Moneta could not fetch full webpage text:",
                    data.page_content_error
                );
            }
        })
        .catch(error => {
            console.error(
                "Failed to send bookmark to Moneta:",
                error
            );
        });
    }
);