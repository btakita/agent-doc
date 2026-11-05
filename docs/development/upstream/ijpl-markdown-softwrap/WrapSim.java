import java.util.*;
import org.intellij.markdown.ast.ASTNode;
import org.intellij.markdown.flavours.gfm.GFMFlavourDescriptor;
import org.intellij.markdown.parser.MarkdownParser;

/**
 * Port of IntelliJ 263 SoftWrapEngine + MarkdownLineWrapPositionStrategy (monospace, no folds/inlays),
 * run on the leaf tokens of the IDE's own bundled intellij-markdown GFM parser. Evidence for
 * ../ijpl-markdown-softwrap-classify-forbidden.md.
 *
 * <pre>
 * MD=$IDEA/lib/intellij.libraries.markdown.jar; K=kotlin-stdlib-2.x.jar
 * javac -cp $MD:$K WrapSim.java && java -cp .:$MD:$K WrapSim repro.md 80
 * </pre>
 *
 * Passing a third argument {@code 391} also applies agent-doc's MarkdownBracketWrapStrategy
 * correction (GH #391); that needs agent-doc's compiled MarkdownBracketWrapPolicy on the classpath.
 */
public class WrapSim {
  static boolean plugin391 = false;
  static String text; static List<int[]> toks = new ArrayList<>(); static List<String> types = new ArrayList<>();
  static void walk(ASTNode n) {
    if (n.getStartOffset() == n.getEndOffset()) return;
    if (n.getChildren().isEmpty()) { toks.add(new int[]{n.getStartOffset(), n.getEndOffset()}); types.add(n.getType().getName()); }
    else for (ASTNode c : n.getChildren()) walk(c);
  }
  static int tokAt(int off) { for (int k = 0; k < toks.size(); k++) if (toks.get(k)[0] <= off && off < toks.get(k)[1]) return k; return -1; }
  static int lineStart(int off) { int i = off; while (i > 0 && text.charAt(i - 1) != '\n') i--; return i; }
  static int lineEnd(int off) { int i = off; while (i < text.length() && text.charAt(i) != '\n') i++; return i; }
  static final Set<String> FORBIDDEN = Set.of("URL", "AUTOLINK", "GFM_AUTOLINK", "EMAIL_AUTOLINK", "LINK_TITLE");
  static final Set<String> BRACKETS = Set.of("[", "]", "(", ")");

  static int generic(int start, int end, int maxPref) {
    if (end <= start) return end;
    int mp = maxPref >= end ? end - 1 : maxPref; mp = Math.max(mp, start);
    for (int i = Math.min(mp, text.length() - 1); i > start; i--) {
      char c = text.charAt(i);
      if (c == '\n') return i + 1;
      if (c != ' ' && c != '\t') continue;
      int target = i + 1;
      if (target <= mp) return target; // all weights equal: the largest stored offset wins
    }
    return -1;
  }
  static Integer classify(int pos) {
    int k = tokAt(pos); if (k < 0) return null; String type = types.get(k);
    int ls = lineStart(pos), linkStart = -1;
    for (int j = k; j >= 0 && toks.get(j)[0] >= ls; j--) {
      if (types.get(j).equals("ATX_HEADER")) return -1;
      if (types.get(j).equals("[") && linkStart == -1) linkStart = toks.get(j)[0];
    }
    if (FORBIDDEN.contains(type)) return linkStart == -1 ? toks.get(k)[0] : linkStart;
    if (!BRACKETS.contains(type) || linkStart == -1 || (type.equals("[") && toks.get(k)[0] == pos)) return null;
    int le = lineEnd(pos);
    for (int j = k; j < toks.size() && toks.get(j)[0] < le; j++) if (types.get(j).equals(")")) return linkStart;
    return null;
  }
  static int markdown(int start, int end, int maxPref) {
    int pos = generic(start, end, maxPref); if (pos < 0) return pos;
    Integer f = classify(pos); if (f == null) return pos;
    if (f == -1 || f <= start) return -1;
    int retry = generic(start, f, Math.min(maxPref, f));
    return retry > 0 ? retry : f;
  }
  /** Mirrors agent-doc MarkdownBracketWrapStrategy.correct (GH #391) around the bundled answer. */
  static int strategy391(int start, int end, int maxPref) {
    int delegated = markdown(start, end, maxPref);
    if (!com.github.btakita.agentdoc.split.frontend.MarkdownBracketWrapPolicy.mayHaveBracket(text, start, end)) return delegated;
    int ws = generic(start, end, maxPref);
    final int fs = start, fe = end, fm = maxPref;
    return com.github.btakita.agentdoc.split.frontend.MarkdownBracketWrapPolicy.correct(text, ws, delegated, () -> {
      int lineStart = lineStart(ws);
      if (lineStart < fs) return markdown(lineStart, fe, fm) >= 0;
      return !com.github.btakita.agentdoc.split.frontend.MarkdownBracketWrapPolicy.mayNeverWrapByLayout(text, lineStart);
    });
  }
  static boolean canWrap(int off) { char c = text.charAt(off); return c == ' ' || c == '\t'; }
  static int findWrapPosition(int max, int min) {
    if (canWrap(max)) return max;
    for (int i = 0, off = max; i < 10 && off >= min; i++) { int prev = off - 1; if (canWrap(prev)) return off; off = prev; }
    return -1;
  }
  static int calc(int min, int max) {
    int p = findWrapPosition(max, min); if (p != -1) return p;
    int w = plugin391 ? strategy391(min - 1, max + 1, max + 1) : markdown(min - 1, max + 1, max + 1);
    if (w < 0) return max; if (w < min) return min; if (w > max) return max; return w;
  }
  public static void main(String[] a) throws Exception {
    text = java.nio.file.Files.readString(java.nio.file.Path.of(a[0])); int width = Integer.parseInt(a[1]); plugin391 = a.length > 2 && a[2].equals("391");
    walk(new MarkdownParser(new GFMFlavourDescriptor()).buildMarkdownTreeFromString(text));
    int ls = 0;
    while (ls < text.length()) {
      int le = lineEnd(ls); List<Integer> wraps = new ArrayList<>();
      int indent = 0; while (ls + indent < le && text.charAt(ls + indent) == ' ') indent++;
      int x = 0, min = -1, max = -1, i = ls;
      while (i < le) {
        x += 1;
        if (min < 0) min = i + 1;
        else {
          if (x > width - 1 && max < 0) max = i;
          if (x > width) {
            int w = min >= max ? min : calc(min, max);
            wraps.add(w); min = -1; max = -1; x = indent + 1;
            if (w <= i) { i = w; continue; }
          }
        }
        i++;
      }
      int prev = ls; System.out.println("logical: " + text.substring(ls, le));
      for (int w : wraps) { System.out.println("  |" + text.substring(prev, w) + "|"); prev = w; }
      System.out.println("  |" + text.substring(prev, le) + "|");
      ls = le + 1;
    }
  }
}
