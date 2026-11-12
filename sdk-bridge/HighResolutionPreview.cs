using System.Globalization;
using System.Reflection;
using DYMO.CrossPlatform.Common.ServiceLocator;
using DYMO.LabelAPI;
using DYMO.LabelAPI.Interfaces;
using DYMO.LabelAPI.Skia;
using DymoSDK.Implementations;
using DymoSDK.Interfaces;
using SkiaSharp;

// Optional design rendering. This is not a printer-resolution bitmap and must
// not replace the ordinary DYMO preview as the reference for printed output.
internal static class HighResolutionPreview
{
    private const int Scale = 4;
    private const int MaximumDimension = 4096;
    private const long MaximumPixels = 16_000_000;
    private const int MaximumEncodedBytes = 32 * 1024 * 1024;
    private const int MaximumSerialisedCharacters = 16 * 1024 * 1024;

    // Returns null for an unsupported SDK version, an oversized/unsafe result,
    // or a barcode label. Callers must retain the unmodified standard preview.
    internal static byte[]? TryRender(IDymoLabel label, byte[] standardPreview)
    {
        if (label is not DymoLabel source || standardPreview is not { Length: > 0 and <= MaximumEncodedBytes }) return null;
        try
        {
            var field = typeof(DymoLabel).GetField("_instanceLabel", BindingFlags.Instance | BindingFlags.NonPublic);
            var serializer = typeof(DymoLabel).GetMethod("SerializeLabel", BindingFlags.Instance | BindingFlags.NonPublic);
            if (field?.GetValue(source) is not ILabel<ILayoutManager> original || serializer is null) return null;
            // The SDK renders barcodes with different geometry at higher DPI.
            if (original.LayoutManager.LabelObjects.Any(o => o is IBarcodeObject)) return null;
            // Reject large embedded images before serialising the model, to avoid
            // constructing another large base64 XML string just to fall back.
            long knownCharacters = source.XMLContent?.Length ?? 0;
            foreach (var image in original.LayoutManager.LabelObjects.OfType<IImageObject>())
            {
                knownCharacters += image.Data?.Length ?? 0;
                if (knownCharacters > MaximumSerialisedCharacters) return null;
            }
            if (knownCharacters > MaximumSerialisedCharacters) return null;
            var originalGeometry = Snapshot(original);
            using var sourceData = SKData.CreateCopy(standardPreview);
            using var sourceCodec = SKCodec.Create(sourceData);
            if (sourceCodec is null) return null;
            var width = sourceCodec.Info.Width;
            var height = sourceCodec.Info.Height;
            if (width <= 0 || height <= 0 || width > (MaximumDimension - Scale) / Scale || height > (MaximumDimension - Scale) / Scale ||
                (long)(width * Scale + Scale) * (height * Scale + Scale) > MaximumPixels) return null;

            // Render an SDK-created clone, never the live label used for printing.
            if (serializer.Invoke(source, null) is not string xml || xml.Length > MaximumSerialisedCharacters) return null;
            var clone = source.GetNewLabelInstance();
            if (ReferenceEquals(source, clone)) return null;
            clone.LoadLabelFromXML(xml);
            if (!clone.GetPreviewLabel().AsSpan().SequenceEqual(standardPreview)) return null;
            if (field.GetValue(clone) is not ILabel<ILayoutManager> model || ReferenceEquals(original, model) || Snapshot(model) != originalGeometry) return null;

            var defaultDpi = new Resolution().DPI;
            if (!float.IsFinite(defaultDpi) || defaultDpi <= 0 || defaultDpi * Scale > 1000) return null;
            var resolution = new Resolution(defaultDpi * Scale);
            var factory = ServiceLocator.Instance.CreateTypeFromInterface<IBitmapFactory>();
            using var wrapper = (SkiaDYMOBitmap)factory.Create(model, resolution, false, false);
            var bitmap = wrapper.SkiaBitmap;
            if (bitmap.Width <= 0 || bitmap.Height <= 0 || bitmap.Width > MaximumDimension || bitmap.Height > MaximumDimension ||
                (long)bitmap.Width * bitmap.Height > MaximumPixels || Math.Abs(bitmap.Width - width * Scale) > Scale || Math.Abs(bitmap.Height - height * Scale) > Scale) return null;
            using var canvas = new SKCanvas(bitmap);
            canvas.Clear(SKColors.Transparent);
            var renderer = LabelFactory.CreateRenderer(resolution);
            renderer.Label = model;
            var info = new RenderInfo(null!, true, false)
            {
                DrawingContext = SkiaDrawingContextFactory.CreateDrawingContext(canvas, resolution, true, true)
            };
            renderer.Render(model, info);
            canvas.Flush();
            if (Snapshot(model) != originalGeometry || Snapshot(original) != originalGeometry) return null;
            using var high = SKImage.FromBitmap(bitmap);
            using var png = high.Encode(SKEncodedImageFormat.Png, 100);
            if (png is null || png.Size <= 0 || png.Size > MaximumEncodedBytes) return null;
            return png.ToArray();
        }
        catch (Exception exception) when (exception is not OutOfMemoryException and not StackOverflowException)
        {
            return null;
        }
    }

    private static string Snapshot(ILabel<ILayoutManager> label)
    {
        static string N(float value) => value.ToString("R", CultureInfo.InvariantCulture);
        var bounds = label.LayoutBounds;
        return $"{N(label.LabelSize.Width)},{N(label.LabelSize.Height)};{N(bounds.Location.X)},{N(bounds.Location.Y)},{N(bounds.Size.Width)},{N(bounds.Size.Height)};" +
            string.Join("|", label.LayoutManager.LabelObjects.Select(o => $"{o.GetType().FullName}:{o.Name}:{N(o.Layout.Rect.Location.X)},{N(o.Layout.Rect.Location.Y)},{N(o.Layout.Rect.Size.Width)},{N(o.Layout.Rect.Size.Height)}"));
    }
}
