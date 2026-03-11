#!/usr/bin/env node
import { readFileSync } from "node:fs";
import {
	composeServices,
	compositionHasErrors,
} from "@theguild/federation-composition";
import { parse } from "graphql";

const input = JSON.parse(process.argv[2]);

const services = await Promise.all(
	input.map(async (sg) => {
		let sdl;
		if (sg.schema_path) {
			sdl = readFileSync(sg.schema_path, "utf-8");
		} else {
			const headers = {
				"Content-Type": "application/json",
				...(sg.headers || {}),
			};
			const response = await fetch(sg.url, {
				method: "POST",
				headers,
				body: JSON.stringify({ query: `{ _service { sdl } }` }),
			});
			const json = await response.json();
			sdl = json.data._service.sdl;
		}
		return { name: sg.name, typeDefs: parse(sdl) };
	}),
);

const result = composeServices(services);

if (compositionHasErrors(result)) {
	console.error(JSON.stringify(result.errors, null, 2));
	process.exit(1);
}

process.stdout.write(result.supergraphSdl);
