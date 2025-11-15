/**
 * HomerActor - A simple agent that responds with quotes from the Iliad and Odyssey
 *
 * Implements the session protocol:
 * - Each session maintains its own quote index
 * - Sessions can be created, resumed from state, and saved
 * - Session state is opaque to the extension
 */

import { v4 as uuidv4 } from "uuid";

const HOMER_QUOTES = [
  "Sing, O goddess, the anger of Achilles son of Peleus, that brought countless ills upon the Achaeans.",
  "Tell me, O muse, of that ingenious hero who travelled far and wide after he had sacked the famous town of Troy.",
  "There is nothing more admirable than when two people who see eye to eye keep house as man and wife, confounding their enemies and delighting their friends.",
  "Even his griefs are a joy long after to one that remembers all that he wrought and endured.",
  "The blade itself incites to deeds of violence.",
  "Hateful to me as the gates of Hades is that man who hides one thing in his heart and speaks another.",
  "Be strong, saith my heart; I am a soldier; I have seen worse sights than this.",
  "There is a time for many words, and there is also a time for sleep.",
  "The journey is the thing.",
  "For rarely are sons similar to their fathers: most are worse, and a few are better than their fathers.",
];

interface HomerSessionState {
  quoteIndex: number;
}

interface Session {
  sessionId: string;
  state: HomerSessionState;
}

export class HomerActor {
  private sessions: Map<string, Session> = new Map();

  /**
   * Create a new session
   * @returns Session ID
   */
  createSession(): string {
    const sessionId = uuidv4();
    this.sessions.set(sessionId, {
      sessionId,
      state: { quoteIndex: 0 },
    });
    console.log(`HomerActor: Created session ${sessionId}`);
    return sessionId;
  }

  /**
   * Resume a session from saved state
   * @param sessionId - Session identifier
   * @param state - Saved session state (opaque blob)
   */
  resumeSession(sessionId: string, state: any): void {
    this.sessions.set(sessionId, {
      sessionId,
      state: state as HomerSessionState,
    });
    console.log(`HomerActor: Resumed session ${sessionId} with state:`, state);
  }

  /**
   * Get the current state of a session
   * @param sessionId - Session identifier
   * @returns Session state (opaque blob for extension)
   */
  getSessionState(sessionId: string): any {
    const session = this.sessions.get(sessionId);
    if (!session) {
      throw new Error(`Session ${sessionId} not found`);
    }
    return session.state;
  }

  /**
   * Process a prompt and stream the response in chunks
   * @param sessionId - Session identifier
   * @param prompt - User prompt
   */
  async *processPrompt(
    sessionId: string,
    prompt: string,
  ): AsyncGenerator<string> {
    const session = this.sessions.get(sessionId);
    if (!session) {
      throw new Error(`Session ${sessionId} not found`);
    }

    // Get the next quote in sequence for this session
    const quote = HOMER_QUOTES[session.state.quoteIndex];
    session.state.quoteIndex =
      (session.state.quoteIndex + 1) % HOMER_QUOTES.length;

    // Format the full response
    const fullResponse = `*"${quote}"*\n\n— Homer`;

    // Stream it word by word to simulate progressive output
    const words = fullResponse.split(" ");
    let accumulated = "";

    for (const word of words) {
      accumulated += (accumulated ? " " : "") + word;
      yield accumulated;
      // Small delay to make streaming visible
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
  }
}
