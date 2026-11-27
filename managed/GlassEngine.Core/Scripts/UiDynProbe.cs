using UnityEngine;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Sonda de la UI dinámica (Canvas v2) sobre el texto de puntaje
    /// de UiDynTest: en Start cambia el texto, el tamaño y el color
    /// por las clases nuevas (Text) y mueve el elemento con
    /// RectTransform, anotando los códigos del puente.
    /// </summary>
    public class UiDynTextProbe : GlassBehaviour
    {
        public override void Start()
        {
            int code = RuntimeBridge.UiSetText(EntityId, "UI DINAMICA 42");
            var go = GameObject.GetOrCreate(EntityId, "Score HUD Text");
            var text = go.GetComponent<Text>() ?? go.AddComponent<Text>();
            text.text = "UI DINAMICA 42";
            text.fontSize = 36f;
            text.color = new Color(1f, 0.9f, 0.2f, 1f);
            var rect = go.GetComponent<RectTransform>() ?? go.AddComponent<RectTransform>();
            rect.anchoredPosition = new Vector2(150f, 70f);
            Debug.Log($"[UiDynTextProbe] entidad {EntityId}: UiSetText -> {code}; texto/tamaño/color/posición aplicados.");
        }
    }

    /// <summary>
    /// Sonda de la UI dinámica sobre el icono de UiDynTest: tiñe la
    /// imagen de verde, la mueve y la encoge con Image y
    /// RectTransform, anotando los códigos del puente.
    /// </summary>
    public class UiDynImageProbe : GlassBehaviour
    {
        public override void Start()
        {
            int code = RuntimeBridge.UiSetImageColor(EntityId, 0.25f, 1f, 0.25f, 1f);
            var go = GameObject.GetOrCreate(EntityId, "Icono Prueba");
            var img = go.GetComponent<Image>() ?? go.AddComponent<Image>();
            img.color = new Color(0.25f, 1f, 0.25f, 1f);
            var rect = go.GetComponent<RectTransform>() ?? go.AddComponent<RectTransform>();
            rect.anchoredPosition = new Vector2(-170f, -60f);
            rect.sizeDelta = new Vector2(150f, 150f);
            Debug.Log($"[UiDynImageProbe] entidad {EntityId}: UiSetImageColor -> {code}; tinte/posición/tamaño aplicados.");
        }
    }

    /// <summary>
    /// Sonda de estilos visuales: fondo, radio de borde, borde, opacidad.
    /// </summary>
    public class UiDynStyleProbe : GlassBehaviour
    {
        public override void Start()
        {
            var go = GameObject.GetOrCreate(EntityId, "Panel Estilo");
            var img = go.GetComponent<Image>() ?? go.AddComponent<Image>();

            int c1 = RuntimeBridge.UiSetBackgroundColor(EntityId, 1f, 0.55f, 0f, 1f);
            img.backgroundColor = new Color(1f, 0.55f, 0f, 1f);
            img.borderRadiusTopLeft = 20f;
            img.borderRadiusTopRight = 20f;
            img.borderRadiusBottomRight = 20f;
            img.borderRadiusBottomLeft = 20f;
            img.borderWidth = 3f;
            img.borderColor = Color.white;
            img.opacity = 0.9f;

            Debug.Log($"[UiDynStyleProbe] entidad {EntityId}: fondo/radio/borde/opacidad aplicados (bg->{c1}).");
        }
    }

    /// <summary>
    /// Sonda de estilos de texto: espaciado, desbordamiento, opacidad.
    /// </summary>
    public class UiDynStyleTextProbe : GlassBehaviour
    {
        public override void Start()
        {
            var go = GameObject.GetOrCreate(EntityId, "Texto Estilo");
            var text = go.GetComponent<Text>() ?? go.AddComponent<Text>();

            text.text = "ESTILO C#";
            text.letterSpacing = 6f;
            text.overflow = "hidden";
            text.opacity = 0.8f;
            int c1 = RuntimeBridge.UiSetLetterSpacing(EntityId, 6f);

            Debug.Log($"[UiDynStyleTextProbe] entidad {EntityId}: espaciado/desbordamiento/opacidad aplicados (letra->{c1}).");
        }
    }
}
