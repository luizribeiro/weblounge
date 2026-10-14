fn main() {
    // sqlx::migrate! embeds the migrations, which cargo doesn't track by itself.
    println!("cargo:rerun-if-changed=migrations");
}
