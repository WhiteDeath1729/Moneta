let monetaMenu = null;

// Store the selection BEFORE the user clicks the button
let pendingBookmark = null;


// Detect text selection
document.addEventListener("mouseup", handleSelection);


function handleSelection(event) {

    // Ignore mouseup events coming from our own menu
    if (monetaMenu && monetaMenu.contains(event.target)) {
        return;
    }

    const selection = window.getSelection();

    if (!selection || selection.isCollapsed) {
        hideMenu();
        return;
    }

    const selectedText = selection.toString().trim();

    if (!selectedText) {
        hideMenu();
        return;
    }


    // SAVE EVERYTHING NOW
    pendingBookmark = {
        title: document.title,
        url: window.location.href,
        selected_text: selectedText
    };

    console.log("MONETA: selection captured");
    console.log(pendingBookmark);

    showMenu(selection);
}


function showMenu(selection) {

    hideMenu();

    const range = selection.getRangeAt(0);
    const rect = range.getBoundingClientRect();


    // Create floating menu
    monetaMenu = document.createElement("div");

    monetaMenu.id = "moneta-selection-menu";


    // Create Bookmark button
    const bookmarkButton = document.createElement("button");

    bookmarkButton.textContent = "Bookmark";


    bookmarkButton.addEventListener("click", function(event) {

        // Prevent webpage from handling this click
        event.stopPropagation();

        console.log("MONETA: BOOKMARK BUTTON CLICKED");

        bookmarkSelection();

        hideMenu();
    });


    monetaMenu.appendChild(bookmarkButton);

    document.body.appendChild(monetaMenu);


    // Position menu
    const menuRect = monetaMenu.getBoundingClientRect();

    let left =
        window.scrollX +
        rect.left +
        (rect.width / 2) -
        (menuRect.width / 2);

    let top =
        window.scrollY +
        rect.bottom +
        8;


    // Prevent menu from going outside viewport
    left = Math.max(
        window.scrollX + 8,
        Math.min(
            left,
            window.scrollX +
            window.innerWidth -
            menuRect.width -
            8
        )
    );


    monetaMenu.style.left = `${left}px`;
    monetaMenu.style.top = `${top}px`;
}


function hideMenu() {

    if (monetaMenu) {
        monetaMenu.remove();
        monetaMenu = null;
    }
}


function bookmarkSelection() {

    // IMPORTANT:
    // Do NOT use window.getSelection() here.
    // The browser may have already cleared it.

    if (!pendingBookmark) {

        console.log(
            "MONETA: no pending bookmark"
        );

        return;
    }


    console.log(
        "================================"
    );

    console.log(
        "MONETA: BOOKMARKING"
    );

    console.log(
        "Title:",
        pendingBookmark.title
    );

    console.log(
        "URL:",
        pendingBookmark.url
    );

    console.log(
        "Selected text:",
        pendingBookmark.selected_text
    );

    console.log(
        "================================"
    );


    // Send data to background.js
    chrome.runtime.sendMessage(
        {
            type: "MONETA_BOOKMARK_SELECTION",

            data: pendingBookmark
        },

        function(response) {

            if (chrome.runtime.lastError) {

                console.error(
                    "MONETA MESSAGE ERROR:",
                    chrome.runtime.lastError.message
                );

                return;
            }

            console.log(
                "MONETA BACKGROUND RESPONSE:",
                response
            );
        }
    );


    // Clear stored selection
    pendingBookmark = null;
}