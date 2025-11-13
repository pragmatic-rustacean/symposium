/**
 * HomerActor - A simple agent that responds with quotes from the Iliad and Odyssey
 */

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

export class HomerActor {
  private quoteIndex = 0;

  /**
   * Process a prompt and stream the response in chunks
   */
  async *processPrompt(prompt: string): AsyncGenerator<string> {
    // Get the next quote in sequence
    const quote = HOMER_QUOTES[this.quoteIndex];
    this.quoteIndex = (this.quoteIndex + 1) % HOMER_QUOTES.length;

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
