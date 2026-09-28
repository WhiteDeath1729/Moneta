const statusDot = document.getElementById("statusDot");
const statusText = document.getElementById("statusText");
const countElement = document.getElementById("count");
const syncButton = document.getElementById("syncButton");
const message = document.getElementById("message");


async function getBookmarkCount() {
    try {
        const tree = await chrome.bookmarks.getTree();

        let count = 0;

        function walk(nodes) {
            for (const node of nodes) {
                if (node.url) {
                    count++;
                }

                if (node.children) {
                    walk(node.children);
                }
            }
        }

        walk(tree);

        countElement.textContent = count;

        return count;

    } catch (error) {
        console.error("Failed to read bookmarks:", error);

        countElement.textContent = "—";

        return 0;
    }
}


async function checkMoneta() {
    try {
        const response = await fetch(
            "http://127.0.0.1:8765/health"
        );

        if (!response.ok) {
            throw new Error(
                `Moneta returned HTTP ${response.status}`
            );
        }

        statusDot.className = "dot connected";
        statusText.textContent = "Moneta connected";

    } catch (error) {
        console.error("Moneta connection failed:", error);

        statusDot.className = "dot error";
        statusText.textContent = "Moneta offline";
    }
}


syncButton.addEventListener("click", async () => {

    syncButton.disabled = true;
    syncButton.textContent = "Syncing...";
    message.textContent = "";

    try {

        const result = await chrome.runtime.sendMessage({
            action: "syncBookmarks"
        });

        if (result && result.success) {

            countElement.textContent = result.count;

            message.textContent =
                `Synced ${result.count} bookmarks to Moneta.`;

            statusDot.className = "dot connected";
            statusText.textContent = "Moneta connected";

        } else {

            message.textContent =
                result?.message || "Sync failed.";

            statusDot.className = "dot error";
            statusText.textContent = "Sync failed";
        }

    } catch (error) {

        console.error("Sync error:", error);

        message.textContent = error.message;

        statusDot.className = "dot error";
        statusText.textContent = "Connection failed";
    }

    syncButton.disabled = false;
    syncButton.textContent = "Sync Bookmarks";
});


getBookmarkCount();
checkMoneta();