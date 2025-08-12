use azalea::{Account, ClientBuilder};
use rocket::routes;
use rocket_dyn_templates::{Template, context};

#[rocket::launch]
fn rocket() -> _ {
	rocket::build()
		.mount("/", routes![index])
		.attach(Template::fairing())
}

#[rocket::get("/")]
fn index() -> Template {
	Template::render("index", context! {})
}
