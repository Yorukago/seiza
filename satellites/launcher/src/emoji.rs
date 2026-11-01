//! `:smile` in the launcher: emoji by name. The row copies the emoji (the launcher's font can't draw color glyphs).
// shortcut: a hand-picked list of common ones, not all of Unicode; add lines here, or read a file when it is not enough.
const LIST: &[(&str, &str)] = &[
    ("😀", "grinning smile happy"), ("😂", "joy laugh tears lol"), ("🙂", "slight smile"), ("😉", "wink"), ("😊", "blush smile happy"), ("😍", "heart eyes love"),
    ("😘", "kiss"), ("😎", "cool sunglasses"), ("🤔", "thinking hmm"), ("😐", "neutral meh"), ("🙄", "eye roll"), ("😴", "sleep zzz tired"),
    ("😢", "cry sad tear"), ("😭", "sob crying"), ("😡", "angry mad"), ("😱", "scream scared shock"), ("🥰", "love hearts smile"), ("🤗", "hug"),
    ("😅", "sweat smile nervous"), ("🤣", "rofl laughing"), ("😇", "angel innocent"), ("🥺", "pleading puppy"), ("😏", "smirk"), ("🤯", "mind blown exploding head"),
    ("👍", "thumbs up yes ok like"), ("👎", "thumbs down no dislike"), ("👋", "wave hello bye"), ("🙏", "pray thanks please folded hands"), ("👏", "clap applause"), ("💪", "muscle strong flex"),
    ("🤝", "handshake deal"), ("✌️", "peace victory"), ("🤞", "fingers crossed luck"), ("👀", "eyes look"), ("🫶", "heart hands"), ("🖖", "vulcan spock"),
    ("❤️", "heart love red"), ("💛", "yellow heart"), ("💙", "blue heart"), ("💜", "purple heart"), ("🖤", "black heart"), ("💔", "broken heart"),
    ("✨", "sparkles stars shine"), ("⭐", "star"), ("🌟", "glowing star"), ("🌙", "moon crescent night"), ("☀️", "sun sunny"), ("🌈", "rainbow"),
    ("🔥", "fire hot lit"), ("💧", "droplet water"), ("❄️", "snowflake cold snow"), ("⚡", "lightning zap bolt"), ("🌊", "wave ocean sea"), ("🌸", "cherry blossom flower sakura"),
    ("🌹", "rose flower"), ("🌲", "tree evergreen"), ("🍀", "clover luck"), ("🌍", "earth globe world"), ("🌌", "milky way galaxy night sky"), ("🪐", "planet saturn"),
    ("🐱", "cat"), ("🐶", "dog"), ("🦊", "fox"), ("🐻", "bear"), ("🐧", "penguin linux"), ("🦋", "butterfly"),
    ("🐢", "turtle"), ("🐙", "octopus"), ("🦄", "unicorn"), ("🐝", "bee"), ("🐸", "frog"), ("🐰", "rabbit bunny"),
    ("🍕", "pizza"), ("🍔", "burger"), ("🍣", "sushi"), ("🍜", "ramen noodles"), ("🍙", "onigiri rice ball"), ("🍎", "apple"),
    ("🍓", "strawberry"), ("🍰", "cake"), ("🍪", "cookie"), ("☕", "coffee tea hot drink"), ("🍵", "tea matcha"), ("🍺", "beer"),
    ("🍷", "wine"), ("🎂", "birthday cake"), ("🎉", "party tada celebrate"), ("🎁", "gift present"), ("🎮", "game controller gaming"), ("🎵", "music note"),
    ("🎧", "headphones music"), ("📷", "camera photo"), ("💻", "laptop computer"), ("⌨️", "keyboard"), ("🖥️", "desktop computer monitor"), ("📱", "phone mobile"),
    ("🔒", "lock locked secure"), ("🔑", "key"), ("🔔", "bell notification"), ("📌", "pin pushpin"), ("📎", "paperclip"), ("📝", "memo note write"),
    ("📚", "books reading"), ("🔍", "search magnifier"), ("💡", "idea bulb light"), ("🔧", "wrench tool fix"), ("⚙️", "gear settings"), ("🧪", "test tube experiment"),
    ("🚀", "rocket launch ship"), ("🛸", "ufo flying saucer"), ("🏠", "house home"), ("🚗", "car"), ("✈️", "airplane plane travel"), ("⏰", "alarm clock time"),
    ("✅", "check done yes"), ("❌", "cross no wrong"), ("⚠️", "warning caution"), ("❓", "question"), ("❗", "exclamation important"), ("💯", "hundred perfect"),
    ("➡️", "arrow right"), ("⬅️", "arrow left"), ("⬆️", "arrow up"), ("⬇️", "arrow down"), ("🔴", "red circle"), ("🟢", "green circle"),
    ("💀", "skull dead"), ("👻", "ghost"), ("🤖", "robot bot"), ("👽", "alien"), ("💩", "poop"), ("🧠", "brain"),
];

/// Up to `max` emoji whose names contain every word of `q` (the part after the colon).
pub fn find(q: &str, max: usize) -> Vec<(&'static str, &'static str)> {
    let words: Vec<String> = q.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return LIST.iter().take(max).copied().collect();
    }
    LIST.iter().filter(|(_, n)| words.iter().all(|w| n.contains(w.as_str()))).take(max).copied().collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_by_any_name_word() {
        assert_eq!(super::find("moon", 5)[0].0, "🌙");
        assert!(super::find("heart eyes", 5).iter().any(|e| e.0 == "😍"));
        assert!(super::find("zzzzzz", 5).is_empty());
        assert_eq!(super::find("", 3).len(), 3);
    }
}
