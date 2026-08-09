# SRS Digitial Infrastructure

No half assed PRs. If you use LLMs, please revise the generated code before submitting PR.

## Design Principles

This is with respect to technical design and the technical realization of any graphical design guidance.

Our mission is to propagandize for the seeds of a vanguard that will build the consciousness American working class to seize the means of production.

The revolutionary moment is currently far, so individual comrade capacity is nowhere near full-time, so capacity is limited.

All technical solutions must be "right-sized" for the current moment, but also as simple as possible. This generally means keeping code DRY as possible and using the latest useful technology.

All technical solutions must minimize busy work for all comrades

## Why? How does this fit into our digital infrastructure?

For interested parties, our websites are essentially templates. These give us the capability to trivially create new website instances, consistent styling across aforementioned instances and enable easy article creation for non technical folks via markdown.

There are additional benefits like having all of our articles versioned via git, enabling granular access control to the repository and automating style and grammar checks.

Authors smoothly make propaganda.

Developers can focus on building further infrastructure.

Editor work is streamlined.

### Specifics

Propagandists use accessible markdown editor to create propaganda

They submit this to the bot, which checks their grammar and writing style

Once this passes, editors can do their checks

The website uses `Zola`, which exposes the `tera` (jinja-like) template engine along with a CommonMarkdown-like interface for generating static websites. We use templates and `tailwindcss` via the `@apply` to "apply" tailwind classes and some custom styles to generated HTML elements.

The bot uses the `harper` grammar checker, accessible via pipeline "gate" and `harper-wasm` for the grammar enabled article editing UI. The discord bot uses `Rust` and `discord_sdk` for developer accessibility and consistency with other projects (all of our POW Captcha, `axum`, `pnpm`, `Zola`, `harper`, `tera`, `tailwindcss` use `Rust`). We deploy our Rust webserver to Cloudflare workers via `wasm`. 

#### Dependencies

Install `podman`, `vscode` and the remote extension for vscode that lets you "ssh" into containers. Run `podman compose up` and open the container with vscode to get started.

Instal `rust-analyzer` extension on VS Code to get linting in editor.

If you want to run the web server locally, then run the static file server via `npm run dev` (specify branch with `BRANCH=seattle npm run dev`). This will generate events, build styles and build the markdown into html. It is a bit slow.

To generate events from the source events (historical), run `./scripts/generate-events.sh`

For full control developer flow, run `pnpm run zsrv` in one terminal tab and `pnpm run styles` in another terminal tab. These respectively watch and build templates, markdown and styles to enable rapid iteration. Visit [http://127.0.0.1:1111/](http://127.0.0.1:1111/) to see the website update as you edit files. I recommend you have the `styles` tab active as your browser will tell you where the non-style build fails.

#### Security

DO NOT EVER HAVE A PHYSICAL `.env` file on the worker.

DO NOT LEAK DISCORD API KEY

ANY NEW DEPS WILL GET YOUR PR REJECTED

DO NOT RUN ANY PIPELINE ON PUSH/PR OTHER THAN MAIN WHICH IS BRANCH PROTECTED

#### Deployment

We deploy the website via Cloudflare Workers. We use the `worker` crate and avoid any use of CloudFlare Worker primitives to avoid vendor lock in.

Deploy via `BRANCH=seattle pnpm run deploy`. Currently, the website is deployed [here](https://national-website.redham28.workers.dev/). This will be updated when the deploy is moved to the official SRS Cloudflare account

#### Create New Branch

...the files, that are

Run `BRANCH=newbranch pnpm run create-new-branch`


## Considerations

### Unknowns

- left link at bottom to more recent article, right link at bottom to less recent article
- hook search up


### TODOs

- TODO
