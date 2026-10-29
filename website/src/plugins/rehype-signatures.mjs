// Built-in reference entries are headings that contain only a code span, such as
// ### `math.round(x)`. Mark them so they render as signature callouts.
function textOf(node) {
  if (node.type === 'text') return node.value;
  return (node.children ?? []).map(textOf).join('');
}

function walk(node, visit) {
  visit(node);
  for (const child of node.children ?? []) walk(child, visit);
}

export function rehypeSignatures() {
  return (tree) => {
    walk(tree, (node) => {
      if (node.type !== 'element' || !/^h[2-4]$/.test(node.tagName)) return;
      const kids = (node.children ?? []).filter((c) => !(c.type === 'text' && !c.value.trim()));
      if (kids.length === 1 && kids[0].type === 'element' && kids[0].tagName === 'code' && /^[\w.]+\(.*\)$/.test(textOf(kids[0]))) {
        node.properties = { ...node.properties, className: [...(node.properties?.className ?? []), 'signature'] };
      }
    });
  };
}
