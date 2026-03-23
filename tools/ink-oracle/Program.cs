using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Serialization;
using Ink;
using Ink.Runtime;
using Path = System.IO.Path;

namespace InkOracle;

class Program
{
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        WriteIndented = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.Never,
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
        Converters =
        {
            new StepOutcomeConverter(),
            new EpisodeOutcomeConverter()
        }
    };

    static int Main(string[] args)
    {
        if (args.Length == 0)
        {
            Console.Error.WriteLine("Usage:");
            Console.Error.WriteLine("  ink-oracle <story.ink> [--output-dir <dir>]");
            Console.Error.WriteLine("  ink-oracle --crawl <tests-dir> [--force]");
            return 1;
        }

        if (args[0] == "--crawl")
        {
            if (args.Length < 2)
            {
                Console.Error.WriteLine("Usage: ink-oracle --crawl <tests-dir> [--force]");
                return 1;
            }
            bool force = args.Contains("--force");
            return CrawlDirectory(args[1], force);
        }

        return ProcessSingleFile(args);
    }

    static int ProcessSingleFile(string[] args)
    {
        var inkPath = args[0];
        string? outputDir = null;

        for (int i = 1; i < args.Length - 1; i++)
        {
            if (args[i] == "--output-dir")
            {
                outputDir = args[i + 1];
            }
        }

        if (!File.Exists(inkPath))
        {
            Console.Error.WriteLine($"File not found: {inkPath}");
            return 1;
        }

        return GenerateOracle(inkPath, outputDir);
    }

    static int GenerateOracle(string inkPath, string? outputDir)
    {
        inkPath = Path.GetFullPath(inkPath);
        var inkSource = File.ReadAllText(inkPath);
        var storyDir = Path.GetDirectoryName(inkPath)!;
        outputDir ??= Path.Combine(storyDir, "oracle");

        // Compile with inklecate.
        Story story;
        try
        {
            var compiler = new Compiler(inkSource, new Compiler.Options
            {
                sourceFilename = inkPath,
                fileHandler = new StoryDirFileHandler(storyDir),
                errorHandler = (string message, ErrorType type) =>
                {
                    if (type == ErrorType.Error)
                    {
                        Console.Error.WriteLine($"  Compile error: {message}");
                    }
                }
            });
            story = compiler.Compile();

            if (story == null)
            {
                Console.Error.WriteLine($"  COMPILE FAILED: {inkPath}");
                return 1;
            }
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"  COMPILE FAILED: {inkPath}: {ex.Message}");
            return 1;
        }

        // Enable ink fallbacks for external functions so the oracle
        // produces output matching what brink does (brink always runs
        // ink fallbacks when no external handler is registered).
        story.allowExternalFunctionFallbacks = true;

        // Explore all branches.
        List<OracleEpisode> episodes;
        try
        {
            var explorer = new Explorer(story);
            episodes = explorer.Explore();
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"  EXPLORE FAILED: {inkPath}: {ex.Message}");
            return 1;
        }

        // Write output.
        Directory.CreateDirectory(outputDir);

        // Clean old oracle files first.
        foreach (var old in Directory.GetFiles(outputDir, "*.oracle.json"))
        {
            File.Delete(old);
        }

        for (int i = 0; i < episodes.Count; i++)
        {
            var outputPath = Path.Combine(outputDir, $"e{i}.oracle.json");
            var json = JsonSerializer.Serialize(episodes[i], JsonOptions);
            File.WriteAllText(outputPath, json + "\n");
        }

        Console.Error.WriteLine($"  OK: {episodes.Count} episodes -> {outputDir}");
        return 0;
    }

    static int CrawlDirectory(string rootDir, bool force)
    {
        if (!Directory.Exists(rootDir))
        {
            Console.Error.WriteLine($"Directory not found: {rootDir}");
            return 1;
        }

        var inkFiles = Directory.GetFiles(rootDir, "story.ink", SearchOption.AllDirectories)
            .OrderBy(f => f)
            .ToList();

        Console.Error.WriteLine($"Found {inkFiles.Count} story.ink files");

        // Get the path to our own executable for subprocess invocation.
        var selfExe = Environment.ProcessPath!;

        int succeeded = 0;
        int failed = 0;
        int skipped = 0;

        foreach (var inkPath in inkFiles)
        {
            var storyDir = Path.GetDirectoryName(inkPath)!;
            var oracleDir = Path.Combine(storyDir, "oracle");

            // Skip if oracle dir exists and not forcing.
            if (!force && Directory.Exists(oracleDir) &&
                Directory.GetFiles(oracleDir, "*.oracle.json").Length > 0)
            {
                skipped++;
                continue;
            }

            var label = $"[{succeeded + failed + skipped + 1}/{inkFiles.Count}] {GetRelativePath(rootDir, inkPath)}";
            Console.Error.Write(label);

            // Run each test as a subprocess to isolate StackOverflow crashes.
            var psi = new System.Diagnostics.ProcessStartInfo
            {
                FileName = selfExe,
                Arguments = $"\"{inkPath}\"",
                RedirectStandardError = true,
                RedirectStandardOutput = true,
                UseShellExecute = false
            };

            using var proc = System.Diagnostics.Process.Start(psi)!;
            proc.WaitForExit(30_000); // 30 second timeout per test.

            if (!proc.HasExited)
            {
                proc.Kill();
                Console.Error.WriteLine($"  TIMEOUT: {inkPath}");
                failed++;
            }
            else if (proc.ExitCode == 0)
            {
                // Print the OK line from stderr.
                var stderr = proc.StandardError.ReadToEnd().TrimEnd();
                Console.Error.WriteLine(stderr.Contains("OK:") ? stderr[stderr.IndexOf("OK:")..] : "");
                succeeded++;
            }
            else
            {
                var stderr = proc.StandardError.ReadToEnd().TrimEnd();
                if (string.IsNullOrEmpty(stderr))
                {
                    Console.Error.WriteLine($"  CRASHED (exit code {proc.ExitCode})");
                }
                else
                {
                    // Print just the first error line.
                    var firstLine = stderr.Split('\n').FirstOrDefault(l => l.Contains("FAILED") || l.Contains("error")) ?? stderr.Split('\n')[0];
                    Console.Error.WriteLine(firstLine);
                }
                failed++;
            }
        }

        Console.Error.WriteLine();
        Console.Error.WriteLine($"Done: {succeeded} succeeded, {failed} failed, {skipped} skipped");

        return failed > 0 ? 1 : 0;
    }

    static string GetRelativePath(string basePath, string fullPath)
    {
        var baseUri = new Uri(Path.GetFullPath(basePath) + Path.DirectorySeparatorChar);
        var fullUri = new Uri(Path.GetFullPath(fullPath));
        return Uri.UnescapeDataString(baseUri.MakeRelativeUri(fullUri).ToString())
            .Replace('/', Path.DirectorySeparatorChar);
    }
}

/// <summary>
/// File handler that resolves includes relative to the story's directory.
/// </summary>
class StoryDirFileHandler : Ink.IFileHandler
{
    private readonly string _storyDir;

    public StoryDirFileHandler(string storyDir)
    {
        _storyDir = storyDir;
    }

    public string ResolveInkFilename(string includeName)
    {
        return Path.Combine(_storyDir, includeName);
    }

    public string LoadInkFileContents(string fullFilename)
    {
        return File.ReadAllText(fullFilename);
    }
}
