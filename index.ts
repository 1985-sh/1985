export class Error1985 extends Error {
  constructor(public status: number, message: string) {
    super(message)
  }
}

export type Model = { id: string; name: string; pricing: { input: number; cached_input: number; output: number; unit: string }; free: boolean }
export type Result = { title: string; url: string; text: string }
export type Event = { type?: string; event?: string; delta?: string; [k: string]: unknown }

export class Client {
  key?: string
  baseUrl: string
  constructor(opts: { key?: string; baseUrl?: string } = {}) {
    const env = (globalThis as any).process?.env ?? {}
    this.key = opts.key ?? env.NINETEEN85_KEY
    this.baseUrl = (opts.baseUrl ?? env.NINETEEN85_URL ?? 'https://api.1985.sh/v1').replace(/\/$/, '')
  }

  private async req(method: string, path: string, body?: unknown): Promise<Response> {
    const r = await fetch(this.baseUrl + path, {
      method,
      headers: { 'content-type': 'application/json', ...(this.key ? { authorization: 'Bearer ' + this.key } : {}) },
      body: body === undefined ? undefined : JSON.stringify(body),
    })
    if (!r.ok) throw new Error1985(r.status, ((await r.json().catch(() => ({}))) as any).error?.message ?? 'error ' + r.status)
    return r
  }

  private async json<T>(method: string, path: string, body?: unknown): Promise<T> {
    return (await this.req(method, path, body)).json() as Promise<T>
  }

  private async *events(path: string, body: unknown): AsyncGenerator<Event> {
    const r = await this.req('POST', path, body)
    const reader = r.body!.getReader()
    const dec = new TextDecoder()
    let buf = ''
    let event = ''
    for (;;) {
      const { value, done } = await reader.read()
      if (done) break
      buf += dec.decode(value, { stream: true })
      const lines = buf.split('\n')
      buf = lines.pop()!
      for (const l of lines) {
        if (l.startsWith('event:')) event = l.slice(6).trim()
        else if (l.startsWith('data:')) {
          const d = l.slice(5).trim()
          if (!d || d === '[DONE]') continue
          const o = JSON.parse(d)
          if (event && o && typeof o === 'object') o.event ??= event
          yield o
        }
      }
    }
  }

  run(model: string, input: unknown, opts: Record<string, unknown> & { stream: false }): Promise<any>
  run(model: string, input: unknown, opts?: Record<string, unknown>): AsyncGenerator<Event>
  run(model: string, input: unknown, opts: Record<string, unknown> = {}): any {
    const body = { model, input, stream: opts.stream ?? true, ...opts }
    return body.stream ? this.events('/responses', body) : this.json('POST', '/responses', body)
  }

  async text(model: string, input: unknown): Promise<string> {
    let s = ''
    for await (const e of this.run(model, input)) if (e.type === 'response.output_text.delta') s += e.delta
    return s
  }

  async search(query: string): Promise<Result[]> {
    return (await this.json<{ results: Result[] }>('POST', '/search', { query })).results
  }

  async sandbox(): Promise<Sandbox> {
    return new Sandbox(this, (await this.json<{ id: string }>('POST', '/sandboxes', {})).id)
  }

  agent(goal: string, model?: string): AsyncGenerator<Event> {
    return this.events('/agents', { goal, ...(model ? { model } : {}) })
  }

  async models(): Promise<Model[]> {
    return (await this.json<{ data: Model[] }>('GET', '/models')).data
  }

  async refresh(): Promise<string> {
    return (this.key = (await this.json<{ key: string }>('POST', '/keys', {})).key)
  }

  async permit(address: string, value: bigint | string, deadline: number, signature: string): Promise<string> {
    return (this.key = (await this.json<{ key: string }>('POST', '/keys', { address, value: String(value), deadline, signature })).key)
  }

  _call<T>(method: string, path: string, body?: unknown): Promise<T> {
    return this.json<T>(method, path, body)
  }
}

export class Sandbox {
  constructor(private client: Client, public id: string) {}
  exec(command: string, timeout = 60): Promise<{ exit_code: number; output: string }> {
    return this.client._call('POST', '/sandboxes/' + this.id + '/exec', { command, timeout })
  }
  close(): Promise<unknown> {
    return this.client._call('DELETE', '/sandboxes/' + this.id)
  }
}
