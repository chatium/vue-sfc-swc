//! Port of `shared/src/domTagConfig.ts`.

use std::collections::HashSet;
use std::sync::LazyLock;

const HTML_TAGS: &str = "html,body,base,head,link,meta,style,title,address,article,aside,footer,\
header,hgroup,h1,h2,h3,h4,h5,h6,nav,section,div,dd,dl,dt,figcaption,\
figure,picture,hr,img,li,main,ol,p,pre,ul,a,b,abbr,bdi,bdo,br,cite,code,\
data,dfn,em,i,kbd,mark,q,rp,rt,ruby,s,samp,small,span,strong,sub,sup,\
time,u,var,wbr,area,audio,map,track,video,embed,object,param,source,\
canvas,script,noscript,del,ins,caption,col,colgroup,table,thead,tbody,td,\
th,tr,button,datalist,fieldset,form,input,label,legend,meter,optgroup,\
option,output,progress,select,textarea,details,dialog,menu,\
summary,template,blockquote,iframe,tfoot";

const SVG_TAGS: &str = "svg,animate,animateMotion,animateTransform,circle,clipPath,color-profile,\
defs,desc,discard,ellipse,feBlend,feColorMatrix,feComponentTransfer,\
feComposite,feConvolveMatrix,feDiffuseLighting,feDisplacementMap,\
feDistantLight,feDropShadow,feFlood,feFuncA,feFuncB,feFuncG,feFuncR,\
feGaussianBlur,feImage,feMerge,feMergeNode,feMorphology,feOffset,\
fePointLight,feSpecularLighting,feSpotLight,feTile,feTurbulence,filter,\
foreignObject,g,hatch,hatchpath,image,line,linearGradient,marker,mask,\
mesh,meshgradient,meshpatch,meshrow,metadata,mpath,path,pattern,\
polygon,polyline,radialGradient,rect,set,solidcolor,stop,switch,symbol,\
text,textPath,title,tspan,unknown,use,view";

const MATH_TAGS: &str = "annotation,annotation-xml,maction,maligngroup,malignmark,math,menclose,\
merror,mfenced,mfrac,mfraction,mglyph,mi,mlabeledtr,mlongdiv,\
mmultiscripts,mn,mo,mover,mpadded,mphantom,mprescripts,mroot,mrow,ms,\
mscarries,mscarry,msgroup,msline,mspace,msqrt,msrow,mstack,mstyle,msub,\
msubsup,msup,mtable,mtd,mtext,mtr,munder,munderover,none,semantics";

const VOID_TAGS: &str = "area,base,br,col,embed,hr,img,input,link,meta,param,source,track,wbr";

fn make_map(s: &'static str) -> HashSet<&'static str> {
    s.split(',').collect()
}

static HTML: LazyLock<HashSet<&'static str>> = LazyLock::new(|| make_map(HTML_TAGS));
static SVG: LazyLock<HashSet<&'static str>> = LazyLock::new(|| make_map(SVG_TAGS));
static MATH: LazyLock<HashSet<&'static str>> = LazyLock::new(|| make_map(MATH_TAGS));
static VOID: LazyLock<HashSet<&'static str>> = LazyLock::new(|| make_map(VOID_TAGS));

pub fn is_html_tag(tag: &str) -> bool {
    HTML.contains(tag)
}
pub fn is_svg_tag(tag: &str) -> bool {
    SVG.contains(tag)
}
pub fn is_math_ml_tag(tag: &str) -> bool {
    MATH.contains(tag)
}
pub fn is_void_tag(tag: &str) -> bool {
    VOID.contains(tag)
}
