-- Your SQL goes here
CREATE TABLE "users"(
	"id" SERIAL NOT NULL PRIMARY KEY,
	"username" VARCHAR NOT NULL,
	"hashed_pwd" VARCHAR NOT NULL,
	"registration_date" TIMESTAMP NOT NULL,
	"email" VARCHAR NOT NULL
);

