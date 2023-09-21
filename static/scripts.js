const apiUrl = `${window.location.protocol}//${window.location.host}`;
const tbody = document.getElementById("tweets-body");
const hideArchivedCheckbox = document.getElementById("hide-archived");
const hideCategorizedCheckbox = document.getElementById("hide-categorized");
const textSearch = document.getElementById("text-search");
const clearSearch = document.getElementById("clear-search");
const tweetPreviewDiv = document.getElementById('tweet-preview');
const title = document.getElementById("title");

const pageSize = 20;

let currentPage = 1;
let isLoading = false;
let isBottomed = false;

window.addEventListener('scroll', () => {
    if (window.innerHeight + window.scrollY >= document.body.offsetHeight - 500 && !isLoading && !isBottomed) {
        fetchTweets();
    }
});


function reload() {
    if (!isLoading) {
        // Reset the current page
        currentPage = 1;
        isBottomed = false;

        // Clear the current list of tweets
        tbody.innerHTML = '';

        // Fetch tweets based on the checkbox state
        updateInfo().then(fetchTweets);
    }
}

hideArchivedCheckbox.addEventListener('change', reload);
hideCategorizedCheckbox.addEventListener('change', reload);
textSearch.addEventListener("input", reload);
clearSearch.addEventListener("click", () => {
    textSearch.value = '';
    reload();
})

// Fetch categories
refreshCategories().then(updateInfo).then(fetchTweets);

function refreshCategories() {
    return fetch(`${apiUrl}/categories`)
        .then(response => response.json())
        .then(data => {
            const categoriesDatalist = document.getElementById("categories");
            categoriesDatalist.innerHTML = '';
            data.forEach(category => {
                const option = document.createElement("option");
                option.value = category;
                categoriesDatalist.appendChild(option);
            });
        });
}

function highlighted(matched) {
    return `<span style="background: yellow">${matched}</span>`;
}

function fetchTweets() {
    hideArchived = hideArchivedCheckbox.checked;
    hideCategorized = hideCategorizedCheckbox.checked;
    text = textSearch.value;

    if (isLoading) return; // Prevent fetching if already fetching

    isLoading = true;

    title.style = "background: aquamarine";

    let url = `${apiUrl}/tweets?page_number=${currentPage}&page_size=${pageSize}&hide_archived=${hideArchived}&hide_categorized=${hideCategorized}`;

    if (text !== undefined && text !== '') {
        url = `${url}&search=${text}`;
    }

    fetch(url)
        .then(response => response.json())
        .then(tweets => {
            tweets.forEach(tweet => {
                const row = document.createElement("tr");

                row.id = tweet.rest_id;

                let full_text = tweet.full_text.replaceAll("\n", "<br>");
                let screen_name = tweet.screen_name;

                if (textSearch.value !== '') {
                    let regex = new RegExp(textSearch.value, 'gi');
                    full_text = full_text.replace(regex, highlighted);
                    screen_name = screen_name.replace(regex, highlighted)
                }

                row.innerHTML = `
                    <td><p>${screen_name}</p> <p>${tweet.created_at}</p> <p>${tweet.rest_id}</p></td>
                    <td>${tweet.liked ? '❤' : ''}${tweet.bookmarked ? '🔖' : ''}</td>
                    <td>${full_text}</td>
                    <td><input list="categories" name="category" placeholder="Select or type a category" onchange="updateTweet('${tweet.rest_id}', this.value)" value="${tweet.category ? tweet.category : ''}"></td>
                    <td><input type="checkbox" name="isImportant" ${tweet.important ? 'checked' : ''} onchange="updateTweet('${tweet.rest_id}', undefined, this.checked, undefined)"></td>
                    <td><input type="checkbox" name="isArchived" ${tweet.archived ? 'checked' : ''} onchange="updateTweet('${tweet.rest_id}', undefined, undefined, this.checked)"></td>
                    <td><button onclick="previewTweet('${tweet.screen_name}', '${tweet.rest_id}')">Preview</button></td>
                `;

                tbody.appendChild(row);
            });

            if (tweets.length < pageSize) {
                isBottomed = true;
                const row = document.createElement("tr");
                row.innerHTML = `<td colspan="7" style="text-align: center">That's all, folks!</td>`;
                tbody.appendChild(row);
            } else {
                currentPage++; // Increment the page for the next fetch
            }

            isLoading = false;

            title.style = '';

        });
}

function updateInfo() {
    return fetch(`${apiUrl}/info`)
        .then(response => response.json())
        .then(info => {
            document.getElementById("stats").innerText = `${info.categorized} + ${info.uncategorized} = ${info.total} tweets; ${info.important} important; ${info.archived} archived.`
        });
}

function updateTweet(id, category, important, archived) {
    const data = {
        category,
        important,
        archived
    };

    let row = document.getElementById(id);
    row.style.backgroundColor = 'aquamarine';

    fetch(`${apiUrl}/tweets/${id}`, {
        method: 'PATCH',
        headers: {
            'Content-Type': 'application/json',
        },
        body: JSON.stringify(data),
    }).then(_ => {
        row.style = undefined;
    }, rejection => {
        console.log(rejection)
        row.style.backgroundColor = 'red';
        setTimeout(reload, 1000);
    }).then(refreshCategories);
}

function previewTweet(screen_name, rest_id) {

    const url = `https://twitter.com/${screen_name}/status/${rest_id}`;

    let bq = document.createElement("blockquote");
    bq.className = "twitter-tweet";

    let a = document.createElement("a");
    a.href = url;

    a.innerText = 'loading...';

    bq.appendChild(a);

    tweetPreviewDiv.innerHTML = '';
    tweetPreviewDiv.appendChild(bq);

    window.twttr.widgets.load();

    reload();

}
