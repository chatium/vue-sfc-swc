//! Port of `shared/src/domAttrConfig.ts` + `escapeHtml.ts`.

use std::collections::HashSet;
use std::sync::LazyLock;

const SPECIAL_BOOLEAN_ATTRS: &str =
    "itemscope,allowfullscreen,formnovalidate,ismap,nomodule,novalidate,readonly";

const BOOLEAN_ATTRS_EXTRA: &str = ",async,autofocus,autoplay,controls,default,defer,disabled,hidden,\
inert,loop,open,required,reversed,scoped,seamless,checked,muted,multiple,selected";

const KNOWN_HTML_ATTR: &str = "accept,accept-charset,accesskey,action,align,allow,alt,async,\
autocapitalize,autocomplete,autofocus,autoplay,background,bgcolor,\
border,buffered,capture,challenge,charset,checked,cite,class,code,\
codebase,color,cols,colspan,content,contenteditable,contextmenu,controls,\
coords,crossorigin,csp,data,datetime,decoding,default,defer,dir,dirname,\
disabled,download,draggable,dropzone,enctype,enterkeyhint,for,form,\
formaction,formenctype,formmethod,formnovalidate,formtarget,headers,\
height,hidden,high,href,hreflang,http-equiv,icon,id,importance,inert,integrity,\
ismap,itemprop,keytype,kind,label,lang,language,loading,list,loop,low,\
manifest,max,maxlength,minlength,media,min,multiple,muted,name,novalidate,\
open,optimum,pattern,ping,placeholder,poster,preload,radiogroup,readonly,\
referrerpolicy,rel,required,reversed,rows,rowspan,sandbox,scope,scoped,\
selected,shape,size,sizes,slot,span,spellcheck,src,srcdoc,srclang,srcset,\
start,step,style,summary,tabindex,target,title,translate,type,usemap,\
value,width,wrap";

const KNOWN_SVG_ATTR: &str = "xmlns,accent-height,accumulate,additive,alignment-baseline,alphabetic,amplitude,\
arabic-form,ascent,attributeName,attributeType,azimuth,baseFrequency,\
baseline-shift,baseProfile,bbox,begin,bias,by,calcMode,cap-height,class,\
clip,clipPathUnits,clip-path,clip-rule,color,color-interpolation,\
color-interpolation-filters,color-profile,color-rendering,\
contentScriptType,contentStyleType,crossorigin,cursor,cx,cy,d,decelerate,\
descent,diffuseConstant,direction,display,divisor,dominant-baseline,dur,dx,\
dy,edgeMode,elevation,enable-background,end,exponent,fill,fill-opacity,\
fill-rule,filter,filterRes,filterUnits,flood-color,flood-opacity,\
font-family,font-size,font-size-adjust,font-stretch,font-style,\
font-variant,font-weight,format,from,fr,fx,fy,g1,g2,glyph-name,\
glyph-orientation-horizontal,glyph-orientation-vertical,glyphRef,\
gradientTransform,gradientUnits,hanging,height,href,hreflang,horiz-adv-x,\
horiz-origin-x,id,ideographic,image-rendering,in,in2,intercept,k,k1,k2,k3,\
k4,kernelMatrix,kernelUnitLength,kerning,keyPoints,keySplines,keyTimes,\
lang,lengthAdjust,letter-spacing,lighting-color,limitingConeAngle,local,\
marker-end,marker-mid,marker-start,markerHeight,markerUnits,markerWidth,\
mask,maskContentUnits,maskUnits,mathematical,max,media,method,min,mode,\
name,numOctaves,offset,opacity,operator,order,orient,orientation,origin,\
overflow,overline-position,overline-thickness,panose-1,paint-order,path,\
pathLength,patternContentUnits,patternTransform,patternUnits,ping,\
pointer-events,points,pointsAtX,pointsAtY,pointsAtZ,preserveAlpha,\
preserveAspectRatio,primitiveUnits,r,radius,referrerPolicy,refX,refY,rel,\
rendering-intent,repeatCount,repeatDur,requiredExtensions,requiredFeatures,\
restart,result,rotate,rx,ry,scale,seed,shape-rendering,slope,spacing,\
specularConstant,specularExponent,speed,spreadMethod,startOffset,\
stdDeviation,stemh,stemv,stitchTiles,stop-color,stop-opacity,\
strikethrough-position,strikethrough-thickness,string,stroke,\
stroke-dasharray,stroke-dashoffset,stroke-linecap,stroke-linejoin,\
stroke-miterlimit,stroke-opacity,stroke-width,style,surfaceScale,\
systemLanguage,tabindex,tableValues,target,targetX,targetY,text-anchor,\
text-decoration,text-rendering,textLength,to,transform,transform-origin,\
type,u1,u2,underline-position,underline-thickness,unicode,unicode-bidi,\
unicode-range,units-per-em,v-alphabetic,v-hanging,v-ideographic,\
v-mathematical,values,vector-effect,version,vert-adv-y,vert-origin-x,\
vert-origin-y,viewBox,viewTarget,visibility,width,widths,word-spacing,\
writing-mode,x,x-height,x1,x2,xChannelSelector,xlink:actuate,xlink:arcrole,\
xlink:href,xlink:role,xlink:show,xlink:title,xlink:type,xmlns:xlink,xml:base,xml:lang,\
xml:space,y,y1,y2,yChannelSelector,z,zoomAndPan";

const KNOWN_MATHML_ATTR: &str = "accent,accentunder,actiontype,align,alignmentscope,altimg,altimg-height,\
altimg-valign,altimg-width,alttext,bevelled,close,columnsalign,columnlines,\
columnspan,denomalign,depth,dir,display,displaystyle,encoding,\
equalcolumns,equalrows,fence,fontstyle,fontweight,form,frame,framespacing,\
groupalign,height,href,id,indentalign,indentalignfirst,indentalignlast,\
indentshift,indentshiftfirst,indentshiftlast,indextype,justify,\
largetop,largeop,lquote,lspace,mathbackground,mathcolor,mathsize,\
mathvariant,maxsize,minlabelspacing,mode,other,overflow,position,\
rowalign,rowlines,rowspan,rquote,rspace,scriptlevel,scriptminsize,\
scriptsizemultiplier,selection,separator,separators,shift,side,\
src,stackalign,stretchy,subscriptshift,superscriptshift,symmetric,\
voffset,width,widths,xlink:href,xlink:show,xlink:type,xmlns";

fn make_map(s: &'static str) -> HashSet<&'static str> {
    s.split(',').collect()
}

static BOOLEAN_ATTRS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    let mut set = make_map(SPECIAL_BOOLEAN_ATTRS);
    set.extend(BOOLEAN_ATTRS_EXTRA.trim_start_matches(',').split(','));
    set
});
static HTML_ATTRS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| make_map(KNOWN_HTML_ATTR));
static SVG_ATTRS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| make_map(KNOWN_SVG_ATTR));
static MATHML_ATTRS: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| make_map(KNOWN_MATHML_ATTR));

pub fn is_boolean_attr(k: &str) -> bool {
    BOOLEAN_ATTRS.contains(k)
}
pub fn is_known_html_attr(k: &str) -> bool {
    HTML_ATTRS.contains(k)
}
pub fn is_known_svg_attr(k: &str) -> bool {
    SVG_ATTRS.contains(k)
}
pub fn is_known_math_ml_attr(k: &str) -> bool {
    MATHML_ATTRS.contains(k)
}

pub fn escape_html(s: &str) -> String {
    if !s.contains(['"', '\'', '&', '<', '>']) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("&quot;"),
            '&' => out.push_str("&amp;"),
            '\'' => out.push_str("&#39;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}
