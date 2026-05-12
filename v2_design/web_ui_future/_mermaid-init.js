// Shared Mermaid init for all v2_design diagrams.
// Loads from CDN, then renders any .mermaid blocks with the dark theme.
(function () {
  const s = document.createElement('script');
  s.src = 'https://cdn.jsdelivr.net/npm/mermaid@10.9.1/dist/mermaid.min.js';
  s.onload = function () {
    window.mermaid.initialize({
      startOnLoad: true,
      theme: 'dark',
      themeVariables: {
        background: '#0b0f17',
        primaryColor: '#11161f',
        primaryTextColor: '#e5e7eb',
        primaryBorderColor: '#334155',
        lineColor: '#64748b',
        secondaryColor: '#161c27',
        tertiaryColor: '#0b0f17',
        nodeBorder: '#334155',
        clusterBkg: '#0e131c',
        clusterBorder: '#1f2937',
        edgeLabelBackground: '#11161f',
        fontFamily: 'ui-monospace, "JetBrains Mono", Menlo, monospace'
      },
      flowchart: { htmlLabels: true, curve: 'basis' },
      sequence: { useMaxWidth: true },
      securityLevel: 'loose'
    });
  };
  document.head.appendChild(s);
})();
