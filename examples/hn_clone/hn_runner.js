// AIPL Full Hacker News Engine Bridge with Authentication & Database Persistence
class AIPLHackerNewsFullHost {
    constructor() {
        this.wasmInstance = null;
        this.currentUser = null;
        this.usersKey = "aipl_hn_users";
        this.storiesKey = "aipl_hn_stories";
        this.sessionKey = "aipl_hn_session";

        this.initDatabase();
    }

    initDatabase() {
        if (!localStorage.getItem(this.usersKey)) {
            // Default seed users
            const defaultUsers = {
                "demis_h": { id: 101, username: "demis_h", passHash: 148291, salt: 991, karma: 482, createdAt: "2026-01-15" },
                "antigravity_dev": { id: 102, username: "antigravity_dev", passHash: 88219, salt: 442, karma: 395, createdAt: "2026-02-01" },
                "matt_dev": { id: 103, username: "matt_dev", passHash: 99120, salt: 123, karma: 189, createdAt: "2026-03-10" }
            };
            localStorage.setItem(this.usersKey, JSON.stringify(defaultUsers));
        }

        if (!localStorage.getItem(this.storiesKey)) {
            const defaultStories = [
                {
                    id: 1,
                    title: "Google DeepMind Announces Gemini 3.6 with Native AIPL Agent Execution Engine",
                    url: "https://deepmind.google/technologies/gemini/",
                    domain: "deepmind.google",
                    points: 482,
                    author: "demis_h",
                    timeAgo: "2 hours ago",
                    comments: 184,
                    category: "top",
                    timestamp: Date.now() - 7200000
                },
                {
                    id: 2,
                    title: "AIPL: Designing a Machine-Native Programming Language for AI Swarms",
                    url: "https://github.com/ai-swarm/aipl",
                    domain: "github.com/ai-swarm",
                    points: 395,
                    author: "antigravity_dev",
                    timeAgo: "3 hours ago",
                    comments: 112,
                    category: "show",
                    timestamp: Date.now() - 10800000
                },
                {
                    id: 3,
                    title: "Show HN: AIPL Wasm Compiler - Zero-Parse Binary AST Payload Generator",
                    url: "https://aipl.org",
                    domain: "aipl.org",
                    points: 189,
                    author: "matt_dev",
                    timeAgo: "7 hours ago",
                    comments: 42,
                    category: "show",
                    timestamp: Date.now() - 25200000
                }
            ];
            localStorage.setItem(this.storiesKey, JSON.stringify(defaultStories));
        }

        const savedSession = localStorage.getItem(this.sessionKey);
        if (savedSession) {
            try {
                this.currentUser = JSON.parse(savedSession);
            } catch (e) {
                this.currentUser = null;
            }
        }
    }

    async initWasm(wasmUrl = "hn_full_engine.wasm") {
        try {
            const response = await fetch(wasmUrl);
            const bytes = await response.arrayBuffer();
            const importObj = {
                env: {
                    sys_print: (val) => console.log("[AIPL Engine Wasm]:", val)
                }
            };
            const { instance } = await WebAssembly.instantiate(bytes, importObj);
            this.wasmInstance = instance;
            console.log("AIPL Full Engine Wasm Loaded!", instance.exports);
        } catch (e) {
            console.warn("Wasm load fallback (using JS AIPL simulation):", e);
        }
    }

    // Hash password using AIPL Wasm hash_password algorithm
    hashPasswordAipl(rawPassStr, saltInt = 991) {
        let code = 0;
        for (let i = 0; i < rawPassStr.length; i++) {
            code = (code * 31 + rawPassStr.charCodeAt(i)) & 0x7fffffff;
        }
        if (code === 0) code = 12345;

        if (this.wasmInstance && this.wasmInstance.exports.hash_password) {
            const hash = this.wasmInstance.exports.hash_password(code, saltInt);
            console.log(`[AIPL Wasm hash_password("${rawPassStr}", ${saltInt})] -> Hash: ${hash}`);
            return hash;
        }
        return (code ^ (saltInt * 10007)) & 0x7fffffff;
    }

    // Register User Account
    registerUser(username, password) {
        if (!username || !password) return { success: false, error: "Username and password required" };
        const users = JSON.parse(localStorage.getItem(this.usersKey));
        if (users[username]) return { success: false, error: "Username already exists" };

        const salt = Math.floor(Math.random() * 8999) + 1000;
        const passHash = this.hashPasswordAipl(password, salt);
        const newUser = {
            id: Object.keys(users).length + 101,
            username: username,
            passHash: passHash,
            salt: salt,
            karma: 1,
            createdAt: new Date().toISOString().split('T')[0]
        };

        users[username] = newUser;
        localStorage.setItem(this.usersKey, JSON.stringify(users));

        // Auto login
        return this.loginUser(username, password);
    }

    // Login User
    loginUser(username, password) {
        const users = JSON.parse(localStorage.getItem(this.usersKey));
        const user = users[username];
        if (!user) return { success: false, error: "User not found" };

        const computedHash = this.hashPasswordAipl(password, user.salt);
        let valid = false;

        if (this.wasmInstance && this.wasmInstance.exports.verify_password) {
            valid = this.wasmInstance.exports.verify_password(this.hashPasswordAipl(password, user.salt), user.salt, user.passHash) !== 0;
        } else {
            valid = computedHash === user.passHash;
        }

        if (computedHash === user.passHash || valid) {
            let sessionToken = 123456;
            if (this.wasmInstance && this.wasmInstance.exports.generate_session_token) {
                sessionToken = this.wasmInstance.exports.generate_session_token(user.id, user.passHash);
            }

            this.currentUser = {
                id: user.id,
                username: user.username,
                karma: user.karma,
                sessionToken: sessionToken,
                createdAt: user.createdAt
            };
            localStorage.setItem(this.sessionKey, JSON.stringify(this.currentUser));
            return { success: true, user: this.currentUser };
        } else {
            return { success: false, error: "Invalid password" };
        }
    }

    logoutUser() {
        this.currentUser = null;
        localStorage.removeItem(this.sessionKey);
    }

    // Submit Link / Story
    submitStory(title, url, category = "top") {
        if (!this.currentUser) return { success: false, error: "Must be logged in to submit links" };
        if (!title || !url) return { success: false, error: "Title and URL required" };

        let domain = url;
        try {
            domain = new URL(url).hostname.replace("www.", "");
        } catch (e) {
            domain = "external";
        }

        const stories = JSON.parse(localStorage.getItem(this.storiesKey));
        const newStory = {
            id: stories.length + 1,
            title: title,
            url: url,
            domain: domain,
            points: 1,
            author: this.currentUser.username,
            timeAgo: "just now",
            comments: 0,
            category: category,
            timestamp: Date.now()
        };

        stories.unshift(newStory);
        localStorage.setItem(this.storiesKey, JSON.stringify(stories));

        // Award karma to submitter using AIPL add_karma
        this.addKarmaToUser(this.currentUser.username, 1);
        return { success: true, story: newStory };
    }

    // Authenticated Upvote
    upvoteStory(storyId) {
        const stories = JSON.parse(localStorage.getItem(this.storiesKey));
        const story = stories.find(s => s.id === storyId);
        if (!story) return;

        const sessionValid = this.currentUser !== null;
        let newScore = story.points + 1;

        if (this.wasmInstance && this.wasmInstance.exports.upvote_authenticated) {
            newScore = this.wasmInstance.exports.upvote_authenticated(story.points, sessionValid ? 1 : 0);
        }

        if (newScore > story.points) {
            story.points = newScore;
            localStorage.setItem(this.storiesKey, JSON.stringify(stories));
            // Award karma to author
            this.addKarmaToUser(story.author, 1);
            this.render();
        }
    }

    addKarmaToUser(username, delta) {
        const users = JSON.parse(localStorage.getItem(this.usersKey));
        if (users[username]) {
            if (this.wasmInstance && this.wasmInstance.exports.add_karma) {
                users[username].karma = this.wasmInstance.exports.add_karma(users[username].karma, delta);
            } else {
                users[username].karma += delta;
            }
            localStorage.setItem(this.usersKey, JSON.stringify(users));

            if (this.currentUser && this.currentUser.username === username) {
                this.currentUser.karma = users[username].karma;
                localStorage.setItem(this.sessionKey, JSON.stringify(this.currentUser));
            }
        }
    }

    render(filter = 'top', searchQuery = '') {
        const listContainer = document.getElementById("hn-story-list");
        if (!listContainer) return;

        const stories = JSON.parse(localStorage.getItem(this.storiesKey)) || [];
        let filteredStories = stories;

        if (filter !== 'top' && filter !== 'all') {
            filteredStories = filteredStories.filter(s => s.category === filter);
        }

        if (searchQuery.trim() !== '') {
            const q = searchQuery.toLowerCase();
            filteredStories = filteredStories.filter(s =>
                s.title.toLowerCase().includes(q) || s.domain.toLowerCase().includes(q)
            );
        }

        let html = '<table border="0" cellpadding="0" cellspacing="0" class="itemlist">';
        filteredStories.forEach((s, idx) => {
            const rank = idx + 1;
            html += `
            <tr class="athing" id="${s.id}">
                <td align="right" valign="top" class="title"><span class="rank">${rank}.</span></td>
                <td valign="top" class="votelinks">
                    <center>
                        <a id="up_${s.id}" href="javascript:void(0)" onclick="window.AIPLHN.upvoteStory(${s.id})">
                            <div class="votearrow" title="Upvote with AIPL Wasm"></div>
                        </a>
                    </center>
                </td>
                <td class="title">
                    <a href="${s.url}" target="_blank" class="storylink">${this.escapeHtml(s.title)}</a>
                    <span class="sitebit comhead"> (<a href="${s.url}" target="_blank"><span class="sitestr">${s.domain}</span></a>)</span>
                </td>
            </tr>
            <tr>
                <td colspan="2"></td>
                <td class="subtext">
                    <span class="score" id="score_${s.id}">${s.points} points</span> by 
                    <a href="javascript:void(0)" onclick="window.AIPLHN.showUserProfile('${s.author}')" class="hnuser">${s.author}</a> 
                    <span class="age"><a href="#">${s.timeAgo}</a></span> | 
                    <a href="#">hide</a> | 
                    <a href="#">${s.comments === 0 ? 'discuss' : s.comments + ' comments'}</a>
                </td>
            </tr>
            <tr class="spacer" style="height:5px"></tr>
            `;
        });
        html += '</table>';
        listContainer.innerHTML = html;

        this.updateNavHeader();
    }

    updateNavHeader() {
        const navRight = document.getElementById("hn-nav-user-area");
        if (!navRight) return;

        if (this.currentUser) {
            navRight.innerHTML = `
                <span class="user-badge">${this.escapeHtml(this.currentUser.username)} (${this.currentUser.karma} karma)</span> | 
                <a href="javascript:void(0)" onclick="showSubmitView()">submit</a> | 
                <a href="javascript:void(0)" onclick="window.AIPLHN.showUserProfile('${this.currentUser.username}')">profile</a> | 
                <a href="javascript:void(0)" onclick="window.AIPLHN.logoutUser(); window.AIPLHN.render();">logout</a>
            `;
        } else {
            navRight.innerHTML = `
                <a href="javascript:void(0)" onclick="showAuthModal('login')">login</a> | 
                <a href="javascript:void(0)" onclick="showAuthModal('register')">create account</a>
            `;
        }
    }

    showUserProfile(username) {
        const users = JSON.parse(localStorage.getItem(this.usersKey));
        const user = users[username];
        if (!user) return alert("User profile not found!");

        const stories = JSON.parse(localStorage.getItem(this.storiesKey));
        const userSubmissions = stories.filter(s => s.author === username);

        let modal = document.getElementById("profile-modal");
        if (!modal) {
            modal = document.createElement("div");
            modal.id = "profile-modal";
            modal.className = "hn-modal";
            document.body.appendChild(modal);
        }

        modal.innerHTML = `
            <div class="hn-modal-content">
                <span class="close-btn" onclick="document.getElementById('profile-modal').style.display='none'">&times;</span>
                <h2>User Profile: ${this.escapeHtml(user.username)}</h2>
                <p><strong>Karma:</strong> <span class="score">${user.karma}</span></p>
                <p><strong>Member Since:</strong> ${user.createdAt}</p>
                <p><strong>Submissions (${userSubmissions.length}):</strong></p>
                <ul>
                    ${userSubmissions.map(s => `<li><a href="${s.url}" target="_blank">${this.escapeHtml(s.title)}</a> (${s.points} points)</li>`).join('')}
                </ul>
            </div>
        `;
        modal.style.display = "block";
    }

    escapeHtml(str) {
        return str.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
    }
}

window.AIPLHN = new AIPLHackerNewsFullHost();
