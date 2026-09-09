The vault is the database.
This module contains the database, its rules and also the classes(descriptions) of its contents such as bookmarks, context, metadata.

Decision Making:

1) What shall the tags type be in the bookmark?
One can use Option<Vec<String>> but this optional declaration raises questions, how does the system actually tag the bookmarks, does it choose the/generate the tags after the request by the user to make a simple bookmark or does it actually do that later, after some sort of request has been made?

The second possibility sounds stupid because what would be the point of the user asking the system to tag the bookmark, it makes much more sense if the system did that on its own.

For the time being we will have to do with an optional declaration and we will see in the future how this will take place/direction.

Ok so the problem here is that we can't use an optional declaration. Because, then we will create the bookmark, and then the user will have to prompt it separately.

Actually maybe we will do the tagging process "separately". We will generate a temporary bookmark and will use AI on top of that. Automatically, without giving any control back to the user about the bookmark.

We can probably use a stop and delete in between of tags generation. Also, since we do a separate tagging process, maybe we can add a feature of possibly blocking AI services.

Task Graph
Extract module-->metadata.rs

Notes:
1) Context is defined independently of bookmark. This has been done to accomodate a changing context that would subsequently follow a changed bookmark.