import requests


BASE_URL = "http://127.0.0.1:8765"


def test_bookmark_submission():
    """
    PY-01
    Objective:
    Verify that Moneta accepts a valid bookmark containing
    student-specific data.
    """

    bookmark = {
        "title": "2024BCS0301 - Rust Research",
        "url": "https://www.rust-lang.org/",
        "folder_path": "Academic/2024BCS0301",
        "created_at_in_chrome": 1727520000
    }

    response = requests.post(
        f"{BASE_URL}/bookmarks",
        json=bookmark
    )

    assert response.status_code in [200, 201, 204]


def test_multiple_bookmark_fields():
    """
    PY-02
    Objective:
    Verify that Moneta accepts a bookmark containing
    the expected bookmark metadata.
    """

    bookmark = {
        "title": "2024BCS0301 - Parallel Computing",
        "url": "https://www.open-mpi.org/",
        "folder_path": "Academic/2024BCS0301/Parallel Computing",
        "created_at_in_chrome": 1727520001
    }

    response = requests.post(
        f"{BASE_URL}/bookmarks",
        json=bookmark
    )

    assert response.status_code in [200, 201, 204]


def test_invalid_bookmark_data():
    """
    PY-03
    Negative test.

    Objective:
    Verify that Moneta does not successfully accept
    an invalid bookmark request.
    """

    invalid_bookmark = {
        "title": "",
        "url": "not-a-valid-url",
        "folder_path": "",
        "created_at_in_chrome": -1
    }

    response = requests.post(
        f"{BASE_URL}/bookmarks",
        json=invalid_bookmark
    )

    assert response.status_code not in [200, 201, 204]


def test_missing_bookmark_fields():
    """
    PY-04
    Negative test.

    Objective:
    Verify that Moneta handles a bookmark request
    with missing required fields.
    """

    incomplete_bookmark = {
        "title": "2024BCS0301 - Incomplete Bookmark"
    }

    response = requests.post(
        f"{BASE_URL}/bookmarks",
        json=incomplete_bookmark
    )

    assert response.status_code not in [200, 201, 204]