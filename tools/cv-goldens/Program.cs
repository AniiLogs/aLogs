// Goldens for crates/alogs-sanctum/src/cv.rs: made-up images through the OpenCV calls Sanctum Helper makes, inputs and
// outputs saved raw (u8 or f32, row-major, channels interleaved) with an index.json. No game pictures.
using System.Text.Json;
using OpenCvSharp;

var outDir = args.Length > 0 ? args[0] : "out";
Directory.CreateDirectory(outDir);
var rnd = new Random(11);
var index = new List<Dictionary<string, object?>>();
int n = 0;

string Save(Mat m)
{
    string name = $"m{n++:D4}.bin";
    var cont = m.IsContinuous() ? m : m.Clone();
    int bytes = (int)(cont.Total() * cont.ElemSize());
    var buf = new byte[bytes];
    System.Runtime.InteropServices.Marshal.Copy(cont.Data, buf, 0, bytes);
    File.WriteAllBytes(Path.Combine(outDir, name), buf);
    return name;
}
object Desc(Mat m) => new Dictionary<string, object> { ["file"] = Save(m), ["w"] = m.Width, ["h"] = m.Height, ["c"] = m.Channels(), ["f32"] = m.Depth() == MatType.CV_32F };
void Add(string op, Dictionary<string, object?> p) { p["op"] = op; index.Add(p); }

// structured made-up pictures: blurred noise with a few filled circles
Mat Picture(int w, int h, int ch)
{
    var m = new Mat(h, w, MatType.CV_8UC(ch));
    var buf = new byte[w * h * ch]; rnd.NextBytes(buf);
    System.Runtime.InteropServices.Marshal.Copy(buf, 0, m.Data, buf.Length);
    Cv2.GaussianBlur(m, m, new Size(0, 0), 1.6);
    for (int i = 0; i < 6; i++)
        Cv2.Circle(m, new Point(rnd.Next(w), rnd.Next(h)), rnd.Next(3, Math.Max(4, w / 5)), new Scalar(rnd.Next(256), rnd.Next(256), rnd.Next(256), rnd.Next(256)), -1);
    return m;
}
Mat Binary(int w, int h)
{
    var m = Picture(w, h, 1); var b = new Mat();
    Cv2.Threshold(m, b, 150, 255, ThresholdTypes.Binary); return b;
}

foreach (var ch in new[] { 1, 3 })
{
    var src = Picture(211, 173, ch);
    foreach (var f in new[] { 0.5, 1 / 1.5, 1 / 2.25, 0.37, 0.8, 1 / 3.0, 1 / 1.333 })
    { var o = new Mat(); Cv2.Resize(src, o, new Size(), f, f, InterpolationFlags.Area); Add("resize", new() { ["in"] = Desc(src), ["fx"] = f, ["interp"] = "area", ["out"] = Desc(o) }); }
    foreach (var (w, h) in new[] { (48, 48), (100, 90), (240, 240) })
    {
        foreach (var it in new[] { InterpolationFlags.Area, InterpolationFlags.Linear, InterpolationFlags.Nearest })
        { var o = new Mat(); Cv2.Resize(src, o, new Size(w, h), 0, 0, it); Add("resize", new() { ["in"] = Desc(src), ["w"] = w, ["h"] = h, ["interp"] = it.ToString().ToLower(), ["out"] = Desc(o) }); }
    }
    foreach (var f in new[] { 1.5, 0.7 })
    { var o = new Mat(); Cv2.Resize(src, o, new Size(), f, f, InterpolationFlags.Linear); Add("resize", new() { ["in"] = Desc(src), ["fx"] = f, ["interp"] = "linear", ["out"] = Desc(o) }); }
    foreach (var f in new[] { 2.0, 2.5 })
    { var o = new Mat(); Cv2.Resize(src, o, new Size(), f, f, InterpolationFlags.Cubic); Add("resize", new() { ["in"] = Desc(src), ["fx"] = f, ["interp"] = "cubic", ["out"] = Desc(o) }); }
}
{
    var bgr = Picture(97, 83, 3); var bgra = Picture(97, 83, 4);
    foreach (var (code, s) in new[] { (ColorConversionCodes.BGR2GRAY, bgr), (ColorConversionCodes.BGRA2GRAY, bgra), (ColorConversionCodes.BGR2HSV, bgr), (ColorConversionCodes.BGRA2BGR, bgra) })
    { var o = new Mat(); Cv2.CvtColor(s, o, code); Add("cvt", new() { ["in"] = Desc(s), ["code"] = code.ToString(), ["out"] = Desc(o) }); }
    var hsv = new Mat(); Cv2.CvtColor(bgr, hsv, ColorConversionCodes.BGR2HSV);
    var r = new Mat(); Cv2.InRange(hsv, new Scalar(0, 0, 141), new Scalar(180, 40, 255), r);
    Add("inrange", new() { ["in"] = Desc(hsv), ["lo"] = new[] { 0, 0, 141 }, ["hi"] = new[] { 180, 40, 255 }, ["out"] = Desc(r) });
}
{
    var g = Picture(120, 100, 1);
    foreach (var (t, mx, ty) in new[] { (120.0, 255.0, ThresholdTypes.Binary), (100.0, 255.0, ThresholdTypes.BinaryInv), (0.0, 255.0, ThresholdTypes.Otsu | ThresholdTypes.Binary) })
    { var o = new Mat(); double got = Cv2.Threshold(g, o, t, mx, ty); Add("threshold", new() { ["in"] = Desc(g), ["t"] = t, ["max"] = mx, ["type"] = ty.ToString(), ["got"] = got, ["out"] = Desc(o) }); }
    var f = new Mat(); g.ConvertTo(f, MatType.CV_32F, 1 / 64.0, -1.5);
    var o2 = new Mat(); Cv2.Threshold(f, o2, 1.0, 0, ThresholdTypes.TozeroInv);
    Add("threshold", new() { ["in"] = Desc(f), ["t"] = 1.0, ["max"] = 0.0, ["type"] = "TozeroInv", ["got"] = 1.0, ["out"] = Desc(o2) });
    foreach (var k in new[] { 3, 5, 9 })
    { var o = new Mat(); Cv2.Blur(f, o, new Size(k, k)); Add("blur", new() { ["in"] = Desc(f), ["k"] = k, ["out"] = Desc(o) }); }
}
{
    var b = Binary(90, 70);
    foreach (var k in new[] { 2, 3, 4, 7, 15 })
    { var o = new Mat(); Cv2.Dilate(b, o, Cv2.GetStructuringElement(MorphShapes.Rect, new Size(k, k))); Add("dilate", new() { ["in"] = Desc(b), ["k"] = k, ["out"] = Desc(o) }); }
    var e = Cv2.GetStructuringElement(MorphShapes.Ellipse, new Size(5, 5));
    foreach (var mt in new[] { MorphTypes.Open, MorphTypes.Gradient })
    { var o = new Mat(); Cv2.MorphologyEx(b, o, mt, e); Add("morph", new() { ["in"] = Desc(b), ["type"] = mt.ToString(), ["out"] = Desc(o) }); }
    var labels = new Mat(); var stats = new Mat(); var cents = new Mat();
    int cnt = Cv2.ConnectedComponentsWithStats(b, labels, stats, cents);
    var st = new List<int[]>();
    for (int i = 0; i < cnt; i++) st.Add([stats.At<int>(i, 0), stats.At<int>(i, 1), stats.At<int>(i, 2), stats.At<int>(i, 3), stats.At<int>(i, 4)]);
    var cs = new List<double[]>();
    for (int i = 0; i < cnt; i++) cs.Add([cents.At<double>(i, 0), cents.At<double>(i, 1)]);
    Add("components", new() { ["in"] = Desc(b), ["n"] = cnt, ["stats"] = st, ["centroids"] = cs });
}
{
    foreach (var ch in new[] { 1, 3 })
    {
        var img = Picture(120, 100, ch);
        var tpl = new Mat(img, new Rect(37, 41, 21, 17)).Clone();
        var noisy = Picture(21, 17, ch); Cv2.AddWeighted(tpl, 0.8, noisy, 0.2, 0, tpl);
        var res = new Mat(); Cv2.MatchTemplate(img, tpl, res, TemplateMatchModes.CCoeffNormed);
        Add("match", new() { ["in"] = Desc(img), ["tpl"] = Desc(tpl), ["out"] = Desc(res) });
        var mask = new Mat(17, 21, MatType.CV_8U, Scalar.All(0)); Cv2.Circle(mask, new Point(10, 8), 7, Scalar.All(255), -1);
        var res2 = new Mat(); Cv2.MatchTemplate(img, tpl, res2, TemplateMatchModes.CCoeffNormed, mask);
        Add("match", new() { ["in"] = Desc(img), ["tpl"] = Desc(tpl), ["mask"] = Desc(mask), ["out"] = Desc(res2) });
    }
}
{
    var g = Picture(200, 160, 1);
    using var c = Cv2.CreateCLAHE(2.0, new Size(8, 8)); var o = new Mat(); c.Apply(g, o);
    Add("clahe", new() { ["in"] = Desc(g), ["out"] = Desc(o) });
    var win = new Mat(); Cv2.CreateHanningWindow(win, new Size(64, 48), MatType.CV_32F);
    Add("hanning", new() { ["w"] = 64, ["h"] = 48, ["out"] = Desc(win) });
    var a = new Mat(); new Mat(g, new Rect(10, 10, 64, 48)).ConvertTo(a, MatType.CV_32F);
    var b = new Mat(); new Mat(g, new Rect(13, 8, 64, 48)).ConvertTo(b, MatType.CV_32F);
    var shift = Cv2.PhaseCorrelate(a, b, win, out double resp);
    Add("phase", new() { ["a"] = Desc(a), ["b"] = Desc(b), ["win"] = Desc(win), ["x"] = shift.X, ["y"] = shift.Y, ["response"] = resp });
    var m = Picture(90, 70, 1);
    var M = Cv2.GetRotationMatrix2D(new Point2f(40, 30), 17, 0.9);
    foreach (var it in new[] { InterpolationFlags.Nearest, InterpolationFlags.Linear })
    {
        var o2 = new Mat(); Cv2.WarpAffine(m, o2, M, m.Size(), it);
        Add("warp", new() { ["in"] = Desc(m), ["M"] = new[] { M.At<double>(0, 0), M.At<double>(0, 1), M.At<double>(0, 2), M.At<double>(1, 0), M.At<double>(1, 1), M.At<double>(1, 2) }, ["interp"] = it.ToString().ToLower(), ["out"] = Desc(o2) });
    }
}
File.WriteAllText(Path.Combine(outDir, "index.json"), JsonSerializer.Serialize(index));
Console.WriteLine($"{index.Count} cases, {n} files");
