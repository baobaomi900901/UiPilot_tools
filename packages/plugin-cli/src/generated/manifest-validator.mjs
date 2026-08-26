"use strict";
export const validate = validate20;
export default validate20;
const schema31 = {"$defs":{"PanelHostKeyDeclaration":{"enum":["ArrowDown","ArrowUp","Primary+N"],"type":"string"},"PublicActivationMode":{"enum":["live","submit"],"type":"string"},"PublicCommandV1":{"additionalProperties":false,"properties":{"activationMode":{"$ref":"#/$defs/PublicActivationMode"},"defaultName":{"type":"string"},"inputPlaceholder":{"default":null,"type":["string","null"]},"inputRequired":{"type":"boolean"},"outputMode":{"$ref":"#/$defs/PublicOutputMode"},"summary":{"default":null,"type":["string","null"]}},"required":["defaultName","activationMode","outputMode","inputRequired"],"type":"object"},"PublicOutputMode":{"enum":["mainResult","window","panel"],"type":"string"},"PublicPanelV1":{"additionalProperties":false,"properties":{"entry":{"type":"string"},"hostKeys":{"default":[],"items":{"$ref":"#/$defs/PanelHostKeyDeclaration"},"maxItems":8,"type":"array","uniqueItems":true}},"required":["entry"],"type":"object"},"PublicPermission":{"enum":["ui.window","ui.panel","clipboard.write","clipboard.read","network.https","files.userSelected","files.index.readAll","notifications.publish","timer.control","background.schedule"],"type":"string"},"PublicPlatform":{"enum":["windows","macos"],"type":"string"},"PublicRuntimeV1":{"additionalProperties":false,"properties":{"entry":{"type":"string"}},"required":["entry"],"type":"object"},"PublicSelectOptionV1":{"additionalProperties":false,"properties":{"label":{"type":"string"},"value":{"type":"string"}},"required":["value","label"],"type":"object"},"PublicSettingV1":{"oneOf":[{"additionalProperties":false,"properties":{"default":{"default":null,"type":["string","null"]},"key":{"type":"string"},"label":{"type":"string"},"type":{"const":"text","type":"string"}},"required":["type","key","label"],"type":"object"},{"additionalProperties":false,"properties":{"key":{"type":"string"},"label":{"type":"string"},"type":{"const":"secret","type":"string"}},"required":["type","key","label"],"type":"object"},{"additionalProperties":false,"properties":{"default":{"default":null,"type":["number","null"]},"key":{"type":"string"},"label":{"type":"string"},"max":{"default":null,"type":["number","null"]},"min":{"default":null,"type":["number","null"]},"step":{"default":null,"type":["number","null"]},"type":{"const":"number","type":"string"}},"required":["type","key","label"],"type":"object"},{"additionalProperties":false,"properties":{"default":{"default":null,"type":["boolean","null"]},"key":{"type":"string"},"label":{"type":"string"},"type":{"const":"boolean","type":"string"}},"required":["type","key","label"],"type":"object"},{"additionalProperties":false,"properties":{"default":{"default":null,"type":["string","null"]},"key":{"type":"string"},"label":{"type":"string"},"options":{"items":{"$ref":"#/$defs/PublicSelectOptionV1"},"type":"array"},"type":{"const":"select","type":"string"}},"required":["type","key","label","options"],"type":"object"}]},"PublicWindowV1":{"additionalProperties":false,"properties":{"entry":{"type":"string"}},"required":["entry"],"type":"object"}},"$schema":"https://json-schema.org/draft/2020-12/schema","additionalProperties":false,"properties":{"apiVersion":{"minimum":0,"type":"integer","maximum":4294967295},"command":{"$ref":"#/$defs/PublicCommandV1"},"description":{"default":null,"type":["string","null"]},"minimumHostVersion":{"type":"string"},"name":{"type":"string"},"panel":{"anyOf":[{"$ref":"#/$defs/PublicPanelV1"},{"type":"null"}],"default":null},"permissions":{"items":{"$ref":"#/$defs/PublicPermission"},"type":"array"},"pluginId":{"type":"string"},"runtime":{"$ref":"#/$defs/PublicRuntimeV1"},"schemaVersion":{"minimum":0,"type":"integer","maximum":4294967295},"settings":{"default":[],"items":{"$ref":"#/$defs/PublicSettingV1"},"type":"array"},"supportedPlatforms":{"items":{"$ref":"#/$defs/PublicPlatform"},"type":"array"},"version":{"type":"string"},"window":{"anyOf":[{"$ref":"#/$defs/PublicWindowV1"},{"type":"null"}],"default":null}},"required":["schemaVersion","pluginId","version","apiVersion","minimumHostVersion","name","supportedPlatforms","command","runtime","permissions"],"title":"PublicManifestV1","type":"object"};
const schema37 = {"enum":["ui.window","ui.panel","clipboard.write","clipboard.read","network.https","files.userSelected","files.index.readAll","notifications.publish","timer.control","background.schedule"],"type":"string"};
const schema38 = {"additionalProperties":false,"properties":{"entry":{"type":"string"}},"required":["entry"],"type":"object"};
const schema41 = {"enum":["windows","macos"],"type":"string"};
const schema42 = {"additionalProperties":false,"properties":{"entry":{"type":"string"}},"required":["entry"],"type":"object"};
const func1 = Object.prototype.hasOwnProperty;
const schema32 = {"additionalProperties":false,"properties":{"activationMode":{"$ref":"#/$defs/PublicActivationMode"},"defaultName":{"type":"string"},"inputPlaceholder":{"default":null,"type":["string","null"]},"inputRequired":{"type":"boolean"},"outputMode":{"$ref":"#/$defs/PublicOutputMode"},"summary":{"default":null,"type":["string","null"]}},"required":["defaultName","activationMode","outputMode","inputRequired"],"type":"object"};
const schema33 = {"enum":["live","submit"],"type":"string"};
const schema34 = {"enum":["mainResult","window","panel"],"type":"string"};

function validate21(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate21.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.defaultName === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "defaultName"},message:"must have required property '"+"defaultName"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.activationMode === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "activationMode"},message:"must have required property '"+"activationMode"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.outputMode === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "outputMode"},message:"must have required property '"+"outputMode"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
if(data.inputRequired === undefined){
const err3 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "inputRequired"},message:"must have required property '"+"inputRequired"+"'"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
for(const key0 in data){
if(!((((((key0 === "activationMode") || (key0 === "defaultName")) || (key0 === "inputPlaceholder")) || (key0 === "inputRequired")) || (key0 === "outputMode")) || (key0 === "summary"))){
const err4 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
}
if(data.activationMode !== undefined){
let data0 = data.activationMode;
if(typeof data0 !== "string"){
const err5 = {instancePath:instancePath+"/activationMode",schemaPath:"#/$defs/PublicActivationMode/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
if(!((data0 === "live") || (data0 === "submit"))){
const err6 = {instancePath:instancePath+"/activationMode",schemaPath:"#/$defs/PublicActivationMode/enum",keyword:"enum",params:{allowedValues: schema33.enum},message:"must be equal to one of the allowed values"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
}
if(data.defaultName !== undefined){
if(typeof data.defaultName !== "string"){
const err7 = {instancePath:instancePath+"/defaultName",schemaPath:"#/properties/defaultName/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
if(data.inputPlaceholder !== undefined){
let data2 = data.inputPlaceholder;
if((typeof data2 !== "string") && (data2 !== null)){
const err8 = {instancePath:instancePath+"/inputPlaceholder",schemaPath:"#/properties/inputPlaceholder/type",keyword:"type",params:{type: schema32.properties.inputPlaceholder.type},message:"must be string,null"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
if(data.inputRequired !== undefined){
if(typeof data.inputRequired !== "boolean"){
const err9 = {instancePath:instancePath+"/inputRequired",schemaPath:"#/properties/inputRequired/type",keyword:"type",params:{type: "boolean"},message:"must be boolean"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
}
if(data.outputMode !== undefined){
let data4 = data.outputMode;
if(typeof data4 !== "string"){
const err10 = {instancePath:instancePath+"/outputMode",schemaPath:"#/$defs/PublicOutputMode/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
if(!(((data4 === "mainResult") || (data4 === "window")) || (data4 === "panel"))){
const err11 = {instancePath:instancePath+"/outputMode",schemaPath:"#/$defs/PublicOutputMode/enum",keyword:"enum",params:{allowedValues: schema34.enum},message:"must be equal to one of the allowed values"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
}
if(data.summary !== undefined){
let data5 = data.summary;
if((typeof data5 !== "string") && (data5 !== null)){
const err12 = {instancePath:instancePath+"/summary",schemaPath:"#/properties/summary/type",keyword:"type",params:{type: schema32.properties.summary.type},message:"must be string,null"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
}
}
else {
const err13 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
validate21.errors = vErrors;
return errors === 0;
}
validate21.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

const schema35 = {"additionalProperties":false,"properties":{"entry":{"type":"string"},"hostKeys":{"default":[],"items":{"$ref":"#/$defs/PanelHostKeyDeclaration"},"maxItems":8,"type":"array","uniqueItems":true}},"required":["entry"],"type":"object"};
const schema36 = {"enum":["ArrowDown","ArrowUp","Primary+N"],"type":"string"};
const func0 = require("ajv/dist/runtime/equal").default;

function validate23(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate23.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.entry === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "entry"},message:"must have required property '"+"entry"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
for(const key0 in data){
if(!((key0 === "entry") || (key0 === "hostKeys"))){
const err1 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
}
if(data.entry !== undefined){
if(typeof data.entry !== "string"){
const err2 = {instancePath:instancePath+"/entry",schemaPath:"#/properties/entry/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
}
if(data.hostKeys !== undefined){
let data1 = data.hostKeys;
if(Array.isArray(data1)){
if(data1.length > 8){
const err3 = {instancePath:instancePath+"/hostKeys",schemaPath:"#/properties/hostKeys/maxItems",keyword:"maxItems",params:{limit: 8},message:"must NOT have more than 8 items"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
const len0 = data1.length;
for(let i0=0; i0<len0; i0++){
let data2 = data1[i0];
if(typeof data2 !== "string"){
const err4 = {instancePath:instancePath+"/hostKeys/" + i0,schemaPath:"#/$defs/PanelHostKeyDeclaration/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
if(!(((data2 === "ArrowDown") || (data2 === "ArrowUp")) || (data2 === "Primary+N"))){
const err5 = {instancePath:instancePath+"/hostKeys/" + i0,schemaPath:"#/$defs/PanelHostKeyDeclaration/enum",keyword:"enum",params:{allowedValues: schema36.enum},message:"must be equal to one of the allowed values"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
}
let i1 = data1.length;
let j0;
if(i1 > 1){
outer0:
for(;i1--;){
for(j0 = i1; j0--;){
if(func0(data1[i1], data1[j0])){
const err6 = {instancePath:instancePath+"/hostKeys",schemaPath:"#/properties/hostKeys/uniqueItems",keyword:"uniqueItems",params:{i: i1, j: j0},message:"must NOT have duplicate items (items ## "+j0+" and "+i1+" are identical)"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
break outer0;
}
}
}
}
}
else {
const err7 = {instancePath:instancePath+"/hostKeys",schemaPath:"#/properties/hostKeys/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
}
else {
const err8 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
validate23.errors = vErrors;
return errors === 0;
}
validate23.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

const schema39 = {"oneOf":[{"additionalProperties":false,"properties":{"default":{"default":null,"type":["string","null"]},"key":{"type":"string"},"label":{"type":"string"},"type":{"const":"text","type":"string"}},"required":["type","key","label"],"type":"object"},{"additionalProperties":false,"properties":{"key":{"type":"string"},"label":{"type":"string"},"type":{"const":"secret","type":"string"}},"required":["type","key","label"],"type":"object"},{"additionalProperties":false,"properties":{"default":{"default":null,"type":["number","null"]},"key":{"type":"string"},"label":{"type":"string"},"max":{"default":null,"type":["number","null"]},"min":{"default":null,"type":["number","null"]},"step":{"default":null,"type":["number","null"]},"type":{"const":"number","type":"string"}},"required":["type","key","label"],"type":"object"},{"additionalProperties":false,"properties":{"default":{"default":null,"type":["boolean","null"]},"key":{"type":"string"},"label":{"type":"string"},"type":{"const":"boolean","type":"string"}},"required":["type","key","label"],"type":"object"},{"additionalProperties":false,"properties":{"default":{"default":null,"type":["string","null"]},"key":{"type":"string"},"label":{"type":"string"},"options":{"items":{"$ref":"#/$defs/PublicSelectOptionV1"},"type":"array"},"type":{"const":"select","type":"string"}},"required":["type","key","label","options"],"type":"object"}]};
const schema40 = {"additionalProperties":false,"properties":{"label":{"type":"string"},"value":{"type":"string"}},"required":["value","label"],"type":"object"};

function validate25(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate25.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
const _errs0 = errors;
let valid0 = false;
let passing0 = null;
const _errs1 = errors;
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.type === undefined){
const err0 = {instancePath,schemaPath:"#/oneOf/0/required",keyword:"required",params:{missingProperty: "type"},message:"must have required property '"+"type"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.key === undefined){
const err1 = {instancePath,schemaPath:"#/oneOf/0/required",keyword:"required",params:{missingProperty: "key"},message:"must have required property '"+"key"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.label === undefined){
const err2 = {instancePath,schemaPath:"#/oneOf/0/required",keyword:"required",params:{missingProperty: "label"},message:"must have required property '"+"label"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
for(const key0 in data){
if(!((((key0 === "default") || (key0 === "key")) || (key0 === "label")) || (key0 === "type"))){
const err3 = {instancePath,schemaPath:"#/oneOf/0/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
}
if(data.default !== undefined){
let data0 = data.default;
if((typeof data0 !== "string") && (data0 !== null)){
const err4 = {instancePath:instancePath+"/default",schemaPath:"#/oneOf/0/properties/default/type",keyword:"type",params:{type: schema39.oneOf[0].properties.default.type},message:"must be string,null"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
}
if(data.key !== undefined){
if(typeof data.key !== "string"){
const err5 = {instancePath:instancePath+"/key",schemaPath:"#/oneOf/0/properties/key/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
}
if(data.label !== undefined){
if(typeof data.label !== "string"){
const err6 = {instancePath:instancePath+"/label",schemaPath:"#/oneOf/0/properties/label/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
}
if(data.type !== undefined){
let data3 = data.type;
if(typeof data3 !== "string"){
const err7 = {instancePath:instancePath+"/type",schemaPath:"#/oneOf/0/properties/type/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
if("text" !== data3){
const err8 = {instancePath:instancePath+"/type",schemaPath:"#/oneOf/0/properties/type/const",keyword:"const",params:{allowedValue: "text"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
}
else {
const err9 = {instancePath,schemaPath:"#/oneOf/0/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
var _valid0 = _errs1 === errors;
if(_valid0){
valid0 = true;
passing0 = 0;
var props0 = true;
}
const _errs12 = errors;
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.type === undefined){
const err10 = {instancePath,schemaPath:"#/oneOf/1/required",keyword:"required",params:{missingProperty: "type"},message:"must have required property '"+"type"+"'"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
if(data.key === undefined){
const err11 = {instancePath,schemaPath:"#/oneOf/1/required",keyword:"required",params:{missingProperty: "key"},message:"must have required property '"+"key"+"'"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
if(data.label === undefined){
const err12 = {instancePath,schemaPath:"#/oneOf/1/required",keyword:"required",params:{missingProperty: "label"},message:"must have required property '"+"label"+"'"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
for(const key1 in data){
if(!(((key1 === "key") || (key1 === "label")) || (key1 === "type"))){
const err13 = {instancePath,schemaPath:"#/oneOf/1/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key1},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
}
if(data.key !== undefined){
if(typeof data.key !== "string"){
const err14 = {instancePath:instancePath+"/key",schemaPath:"#/oneOf/1/properties/key/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err14];
}
else {
vErrors.push(err14);
}
errors++;
}
}
if(data.label !== undefined){
if(typeof data.label !== "string"){
const err15 = {instancePath:instancePath+"/label",schemaPath:"#/oneOf/1/properties/label/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err15];
}
else {
vErrors.push(err15);
}
errors++;
}
}
if(data.type !== undefined){
let data6 = data.type;
if(typeof data6 !== "string"){
const err16 = {instancePath:instancePath+"/type",schemaPath:"#/oneOf/1/properties/type/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err16];
}
else {
vErrors.push(err16);
}
errors++;
}
if("secret" !== data6){
const err17 = {instancePath:instancePath+"/type",schemaPath:"#/oneOf/1/properties/type/const",keyword:"const",params:{allowedValue: "secret"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err17];
}
else {
vErrors.push(err17);
}
errors++;
}
}
}
else {
const err18 = {instancePath,schemaPath:"#/oneOf/1/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err18];
}
else {
vErrors.push(err18);
}
errors++;
}
var _valid0 = _errs12 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 1];
}
else {
if(_valid0){
valid0 = true;
passing0 = 1;
if(props0 !== true){
props0 = true;
}
}
const _errs21 = errors;
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.type === undefined){
const err19 = {instancePath,schemaPath:"#/oneOf/2/required",keyword:"required",params:{missingProperty: "type"},message:"must have required property '"+"type"+"'"};
if(vErrors === null){
vErrors = [err19];
}
else {
vErrors.push(err19);
}
errors++;
}
if(data.key === undefined){
const err20 = {instancePath,schemaPath:"#/oneOf/2/required",keyword:"required",params:{missingProperty: "key"},message:"must have required property '"+"key"+"'"};
if(vErrors === null){
vErrors = [err20];
}
else {
vErrors.push(err20);
}
errors++;
}
if(data.label === undefined){
const err21 = {instancePath,schemaPath:"#/oneOf/2/required",keyword:"required",params:{missingProperty: "label"},message:"must have required property '"+"label"+"'"};
if(vErrors === null){
vErrors = [err21];
}
else {
vErrors.push(err21);
}
errors++;
}
for(const key2 in data){
if(!(((((((key2 === "default") || (key2 === "key")) || (key2 === "label")) || (key2 === "max")) || (key2 === "min")) || (key2 === "step")) || (key2 === "type"))){
const err22 = {instancePath,schemaPath:"#/oneOf/2/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key2},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err22];
}
else {
vErrors.push(err22);
}
errors++;
}
}
if(data.default !== undefined){
let data7 = data.default;
if((!((typeof data7 == "number") && (isFinite(data7)))) && (data7 !== null)){
const err23 = {instancePath:instancePath+"/default",schemaPath:"#/oneOf/2/properties/default/type",keyword:"type",params:{type: schema39.oneOf[2].properties.default.type},message:"must be number,null"};
if(vErrors === null){
vErrors = [err23];
}
else {
vErrors.push(err23);
}
errors++;
}
}
if(data.key !== undefined){
if(typeof data.key !== "string"){
const err24 = {instancePath:instancePath+"/key",schemaPath:"#/oneOf/2/properties/key/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err24];
}
else {
vErrors.push(err24);
}
errors++;
}
}
if(data.label !== undefined){
if(typeof data.label !== "string"){
const err25 = {instancePath:instancePath+"/label",schemaPath:"#/oneOf/2/properties/label/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err25];
}
else {
vErrors.push(err25);
}
errors++;
}
}
if(data.max !== undefined){
let data10 = data.max;
if((!((typeof data10 == "number") && (isFinite(data10)))) && (data10 !== null)){
const err26 = {instancePath:instancePath+"/max",schemaPath:"#/oneOf/2/properties/max/type",keyword:"type",params:{type: schema39.oneOf[2].properties.max.type},message:"must be number,null"};
if(vErrors === null){
vErrors = [err26];
}
else {
vErrors.push(err26);
}
errors++;
}
}
if(data.min !== undefined){
let data11 = data.min;
if((!((typeof data11 == "number") && (isFinite(data11)))) && (data11 !== null)){
const err27 = {instancePath:instancePath+"/min",schemaPath:"#/oneOf/2/properties/min/type",keyword:"type",params:{type: schema39.oneOf[2].properties.min.type},message:"must be number,null"};
if(vErrors === null){
vErrors = [err27];
}
else {
vErrors.push(err27);
}
errors++;
}
}
if(data.step !== undefined){
let data12 = data.step;
if((!((typeof data12 == "number") && (isFinite(data12)))) && (data12 !== null)){
const err28 = {instancePath:instancePath+"/step",schemaPath:"#/oneOf/2/properties/step/type",keyword:"type",params:{type: schema39.oneOf[2].properties.step.type},message:"must be number,null"};
if(vErrors === null){
vErrors = [err28];
}
else {
vErrors.push(err28);
}
errors++;
}
}
if(data.type !== undefined){
let data13 = data.type;
if(typeof data13 !== "string"){
const err29 = {instancePath:instancePath+"/type",schemaPath:"#/oneOf/2/properties/type/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err29];
}
else {
vErrors.push(err29);
}
errors++;
}
if("number" !== data13){
const err30 = {instancePath:instancePath+"/type",schemaPath:"#/oneOf/2/properties/type/const",keyword:"const",params:{allowedValue: "number"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err30];
}
else {
vErrors.push(err30);
}
errors++;
}
}
}
else {
const err31 = {instancePath,schemaPath:"#/oneOf/2/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err31];
}
else {
vErrors.push(err31);
}
errors++;
}
var _valid0 = _errs21 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 2];
}
else {
if(_valid0){
valid0 = true;
passing0 = 2;
if(props0 !== true){
props0 = true;
}
}
const _errs38 = errors;
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.type === undefined){
const err32 = {instancePath,schemaPath:"#/oneOf/3/required",keyword:"required",params:{missingProperty: "type"},message:"must have required property '"+"type"+"'"};
if(vErrors === null){
vErrors = [err32];
}
else {
vErrors.push(err32);
}
errors++;
}
if(data.key === undefined){
const err33 = {instancePath,schemaPath:"#/oneOf/3/required",keyword:"required",params:{missingProperty: "key"},message:"must have required property '"+"key"+"'"};
if(vErrors === null){
vErrors = [err33];
}
else {
vErrors.push(err33);
}
errors++;
}
if(data.label === undefined){
const err34 = {instancePath,schemaPath:"#/oneOf/3/required",keyword:"required",params:{missingProperty: "label"},message:"must have required property '"+"label"+"'"};
if(vErrors === null){
vErrors = [err34];
}
else {
vErrors.push(err34);
}
errors++;
}
for(const key3 in data){
if(!((((key3 === "default") || (key3 === "key")) || (key3 === "label")) || (key3 === "type"))){
const err35 = {instancePath,schemaPath:"#/oneOf/3/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key3},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err35];
}
else {
vErrors.push(err35);
}
errors++;
}
}
if(data.default !== undefined){
let data14 = data.default;
if((typeof data14 !== "boolean") && (data14 !== null)){
const err36 = {instancePath:instancePath+"/default",schemaPath:"#/oneOf/3/properties/default/type",keyword:"type",params:{type: schema39.oneOf[3].properties.default.type},message:"must be boolean,null"};
if(vErrors === null){
vErrors = [err36];
}
else {
vErrors.push(err36);
}
errors++;
}
}
if(data.key !== undefined){
if(typeof data.key !== "string"){
const err37 = {instancePath:instancePath+"/key",schemaPath:"#/oneOf/3/properties/key/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err37];
}
else {
vErrors.push(err37);
}
errors++;
}
}
if(data.label !== undefined){
if(typeof data.label !== "string"){
const err38 = {instancePath:instancePath+"/label",schemaPath:"#/oneOf/3/properties/label/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err38];
}
else {
vErrors.push(err38);
}
errors++;
}
}
if(data.type !== undefined){
let data17 = data.type;
if(typeof data17 !== "string"){
const err39 = {instancePath:instancePath+"/type",schemaPath:"#/oneOf/3/properties/type/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err39];
}
else {
vErrors.push(err39);
}
errors++;
}
if("boolean" !== data17){
const err40 = {instancePath:instancePath+"/type",schemaPath:"#/oneOf/3/properties/type/const",keyword:"const",params:{allowedValue: "boolean"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err40];
}
else {
vErrors.push(err40);
}
errors++;
}
}
}
else {
const err41 = {instancePath,schemaPath:"#/oneOf/3/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err41];
}
else {
vErrors.push(err41);
}
errors++;
}
var _valid0 = _errs38 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 3];
}
else {
if(_valid0){
valid0 = true;
passing0 = 3;
if(props0 !== true){
props0 = true;
}
}
const _errs49 = errors;
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.type === undefined){
const err42 = {instancePath,schemaPath:"#/oneOf/4/required",keyword:"required",params:{missingProperty: "type"},message:"must have required property '"+"type"+"'"};
if(vErrors === null){
vErrors = [err42];
}
else {
vErrors.push(err42);
}
errors++;
}
if(data.key === undefined){
const err43 = {instancePath,schemaPath:"#/oneOf/4/required",keyword:"required",params:{missingProperty: "key"},message:"must have required property '"+"key"+"'"};
if(vErrors === null){
vErrors = [err43];
}
else {
vErrors.push(err43);
}
errors++;
}
if(data.label === undefined){
const err44 = {instancePath,schemaPath:"#/oneOf/4/required",keyword:"required",params:{missingProperty: "label"},message:"must have required property '"+"label"+"'"};
if(vErrors === null){
vErrors = [err44];
}
else {
vErrors.push(err44);
}
errors++;
}
if(data.options === undefined){
const err45 = {instancePath,schemaPath:"#/oneOf/4/required",keyword:"required",params:{missingProperty: "options"},message:"must have required property '"+"options"+"'"};
if(vErrors === null){
vErrors = [err45];
}
else {
vErrors.push(err45);
}
errors++;
}
for(const key4 in data){
if(!(((((key4 === "default") || (key4 === "key")) || (key4 === "label")) || (key4 === "options")) || (key4 === "type"))){
const err46 = {instancePath,schemaPath:"#/oneOf/4/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key4},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err46];
}
else {
vErrors.push(err46);
}
errors++;
}
}
if(data.default !== undefined){
let data18 = data.default;
if((typeof data18 !== "string") && (data18 !== null)){
const err47 = {instancePath:instancePath+"/default",schemaPath:"#/oneOf/4/properties/default/type",keyword:"type",params:{type: schema39.oneOf[4].properties.default.type},message:"must be string,null"};
if(vErrors === null){
vErrors = [err47];
}
else {
vErrors.push(err47);
}
errors++;
}
}
if(data.key !== undefined){
if(typeof data.key !== "string"){
const err48 = {instancePath:instancePath+"/key",schemaPath:"#/oneOf/4/properties/key/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err48];
}
else {
vErrors.push(err48);
}
errors++;
}
}
if(data.label !== undefined){
if(typeof data.label !== "string"){
const err49 = {instancePath:instancePath+"/label",schemaPath:"#/oneOf/4/properties/label/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err49];
}
else {
vErrors.push(err49);
}
errors++;
}
}
if(data.options !== undefined){
let data21 = data.options;
if(Array.isArray(data21)){
const len0 = data21.length;
for(let i0=0; i0<len0; i0++){
let data22 = data21[i0];
if(data22 && typeof data22 == "object" && !Array.isArray(data22)){
if(data22.value === undefined){
const err50 = {instancePath:instancePath+"/options/" + i0,schemaPath:"#/$defs/PublicSelectOptionV1/required",keyword:"required",params:{missingProperty: "value"},message:"must have required property '"+"value"+"'"};
if(vErrors === null){
vErrors = [err50];
}
else {
vErrors.push(err50);
}
errors++;
}
if(data22.label === undefined){
const err51 = {instancePath:instancePath+"/options/" + i0,schemaPath:"#/$defs/PublicSelectOptionV1/required",keyword:"required",params:{missingProperty: "label"},message:"must have required property '"+"label"+"'"};
if(vErrors === null){
vErrors = [err51];
}
else {
vErrors.push(err51);
}
errors++;
}
for(const key5 in data22){
if(!((key5 === "label") || (key5 === "value"))){
const err52 = {instancePath:instancePath+"/options/" + i0,schemaPath:"#/$defs/PublicSelectOptionV1/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key5},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err52];
}
else {
vErrors.push(err52);
}
errors++;
}
}
if(data22.label !== undefined){
if(typeof data22.label !== "string"){
const err53 = {instancePath:instancePath+"/options/" + i0+"/label",schemaPath:"#/$defs/PublicSelectOptionV1/properties/label/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err53];
}
else {
vErrors.push(err53);
}
errors++;
}
}
if(data22.value !== undefined){
if(typeof data22.value !== "string"){
const err54 = {instancePath:instancePath+"/options/" + i0+"/value",schemaPath:"#/$defs/PublicSelectOptionV1/properties/value/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err54];
}
else {
vErrors.push(err54);
}
errors++;
}
}
}
else {
const err55 = {instancePath:instancePath+"/options/" + i0,schemaPath:"#/$defs/PublicSelectOptionV1/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err55];
}
else {
vErrors.push(err55);
}
errors++;
}
}
}
else {
const err56 = {instancePath:instancePath+"/options",schemaPath:"#/oneOf/4/properties/options/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err56];
}
else {
vErrors.push(err56);
}
errors++;
}
}
if(data.type !== undefined){
let data25 = data.type;
if(typeof data25 !== "string"){
const err57 = {instancePath:instancePath+"/type",schemaPath:"#/oneOf/4/properties/type/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err57];
}
else {
vErrors.push(err57);
}
errors++;
}
if("select" !== data25){
const err58 = {instancePath:instancePath+"/type",schemaPath:"#/oneOf/4/properties/type/const",keyword:"const",params:{allowedValue: "select"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err58];
}
else {
vErrors.push(err58);
}
errors++;
}
}
}
else {
const err59 = {instancePath,schemaPath:"#/oneOf/4/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err59];
}
else {
vErrors.push(err59);
}
errors++;
}
var _valid0 = _errs49 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 4];
}
else {
if(_valid0){
valid0 = true;
passing0 = 4;
if(props0 !== true){
props0 = true;
}
}
}
}
}
}
if(!valid0){
const err60 = {instancePath,schemaPath:"#/oneOf",keyword:"oneOf",params:{passingSchemas: passing0},message:"must match exactly one schema in oneOf"};
if(vErrors === null){
vErrors = [err60];
}
else {
vErrors.push(err60);
}
errors++;
}
else {
errors = _errs0;
if(vErrors !== null){
if(_errs0){
vErrors.length = _errs0;
}
else {
vErrors = null;
}
}
}
validate25.errors = vErrors;
evaluated0.props = props0;
return errors === 0;
}
validate25.evaluated = {"dynamicProps":true,"dynamicItems":false};


function validate20(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate20.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.schemaVersion === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "schemaVersion"},message:"must have required property '"+"schemaVersion"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.pluginId === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "pluginId"},message:"must have required property '"+"pluginId"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.version === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "version"},message:"must have required property '"+"version"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
if(data.apiVersion === undefined){
const err3 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "apiVersion"},message:"must have required property '"+"apiVersion"+"'"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
if(data.minimumHostVersion === undefined){
const err4 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "minimumHostVersion"},message:"must have required property '"+"minimumHostVersion"+"'"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
if(data.name === undefined){
const err5 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "name"},message:"must have required property '"+"name"+"'"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
if(data.supportedPlatforms === undefined){
const err6 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "supportedPlatforms"},message:"must have required property '"+"supportedPlatforms"+"'"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
if(data.command === undefined){
const err7 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "command"},message:"must have required property '"+"command"+"'"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
if(data.runtime === undefined){
const err8 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "runtime"},message:"must have required property '"+"runtime"+"'"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
if(data.permissions === undefined){
const err9 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "permissions"},message:"must have required property '"+"permissions"+"'"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
for(const key0 in data){
if(!(func1.call(schema31.properties, key0))){
const err10 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
}
if(data.apiVersion !== undefined){
let data0 = data.apiVersion;
if(!(((typeof data0 == "number") && (!(data0 % 1) && !isNaN(data0))) && (isFinite(data0)))){
const err11 = {instancePath:instancePath+"/apiVersion",schemaPath:"#/properties/apiVersion/type",keyword:"type",params:{type: "integer"},message:"must be integer"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
if((typeof data0 == "number") && (isFinite(data0))){
if(data0 > 4294967295 || isNaN(data0)){
const err12 = {instancePath:instancePath+"/apiVersion",schemaPath:"#/properties/apiVersion/maximum",keyword:"maximum",params:{comparison: "<=", limit: 4294967295},message:"must be <= 4294967295"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
if(data0 < 0 || isNaN(data0)){
const err13 = {instancePath:instancePath+"/apiVersion",schemaPath:"#/properties/apiVersion/minimum",keyword:"minimum",params:{comparison: ">=", limit: 0},message:"must be >= 0"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
}
}
if(data.command !== undefined){
if(!(validate21(data.command, {instancePath:instancePath+"/command",parentData:data,parentDataProperty:"command",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate21.errors : vErrors.concat(validate21.errors);
errors = vErrors.length;
}
}
if(data.description !== undefined){
let data2 = data.description;
if((typeof data2 !== "string") && (data2 !== null)){
const err14 = {instancePath:instancePath+"/description",schemaPath:"#/properties/description/type",keyword:"type",params:{type: schema31.properties.description.type},message:"must be string,null"};
if(vErrors === null){
vErrors = [err14];
}
else {
vErrors.push(err14);
}
errors++;
}
}
if(data.minimumHostVersion !== undefined){
if(typeof data.minimumHostVersion !== "string"){
const err15 = {instancePath:instancePath+"/minimumHostVersion",schemaPath:"#/properties/minimumHostVersion/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err15];
}
else {
vErrors.push(err15);
}
errors++;
}
}
if(data.name !== undefined){
if(typeof data.name !== "string"){
const err16 = {instancePath:instancePath+"/name",schemaPath:"#/properties/name/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err16];
}
else {
vErrors.push(err16);
}
errors++;
}
}
if(data.panel !== undefined){
let data5 = data.panel;
const _errs12 = errors;
let valid1 = false;
const _errs13 = errors;
if(!(validate23(data5, {instancePath:instancePath+"/panel",parentData:data,parentDataProperty:"panel",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate23.errors : vErrors.concat(validate23.errors);
errors = vErrors.length;
}
var _valid0 = _errs13 === errors;
valid1 = valid1 || _valid0;
const _errs14 = errors;
if(data5 !== null){
const err17 = {instancePath:instancePath+"/panel",schemaPath:"#/properties/panel/anyOf/1/type",keyword:"type",params:{type: "null"},message:"must be null"};
if(vErrors === null){
vErrors = [err17];
}
else {
vErrors.push(err17);
}
errors++;
}
var _valid0 = _errs14 === errors;
valid1 = valid1 || _valid0;
if(!valid1){
const err18 = {instancePath:instancePath+"/panel",schemaPath:"#/properties/panel/anyOf",keyword:"anyOf",params:{},message:"must match a schema in anyOf"};
if(vErrors === null){
vErrors = [err18];
}
else {
vErrors.push(err18);
}
errors++;
}
else {
errors = _errs12;
if(vErrors !== null){
if(_errs12){
vErrors.length = _errs12;
}
else {
vErrors = null;
}
}
}
}
if(data.permissions !== undefined){
let data6 = data.permissions;
if(Array.isArray(data6)){
const len0 = data6.length;
for(let i0=0; i0<len0; i0++){
let data7 = data6[i0];
if(typeof data7 !== "string"){
const err19 = {instancePath:instancePath+"/permissions/" + i0,schemaPath:"#/$defs/PublicPermission/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err19];
}
else {
vErrors.push(err19);
}
errors++;
}
if(!((((((((((data7 === "ui.window") || (data7 === "ui.panel")) || (data7 === "clipboard.write")) || (data7 === "clipboard.read")) || (data7 === "network.https")) || (data7 === "files.userSelected")) || (data7 === "files.index.readAll")) || (data7 === "notifications.publish")) || (data7 === "timer.control")) || (data7 === "background.schedule"))){
const err20 = {instancePath:instancePath+"/permissions/" + i0,schemaPath:"#/$defs/PublicPermission/enum",keyword:"enum",params:{allowedValues: schema37.enum},message:"must be equal to one of the allowed values"};
if(vErrors === null){
vErrors = [err20];
}
else {
vErrors.push(err20);
}
errors++;
}
}
}
else {
const err21 = {instancePath:instancePath+"/permissions",schemaPath:"#/properties/permissions/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err21];
}
else {
vErrors.push(err21);
}
errors++;
}
}
if(data.pluginId !== undefined){
if(typeof data.pluginId !== "string"){
const err22 = {instancePath:instancePath+"/pluginId",schemaPath:"#/properties/pluginId/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err22];
}
else {
vErrors.push(err22);
}
errors++;
}
}
if(data.runtime !== undefined){
let data9 = data.runtime;
if(data9 && typeof data9 == "object" && !Array.isArray(data9)){
if(data9.entry === undefined){
const err23 = {instancePath:instancePath+"/runtime",schemaPath:"#/$defs/PublicRuntimeV1/required",keyword:"required",params:{missingProperty: "entry"},message:"must have required property '"+"entry"+"'"};
if(vErrors === null){
vErrors = [err23];
}
else {
vErrors.push(err23);
}
errors++;
}
for(const key1 in data9){
if(!(key1 === "entry")){
const err24 = {instancePath:instancePath+"/runtime",schemaPath:"#/$defs/PublicRuntimeV1/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key1},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err24];
}
else {
vErrors.push(err24);
}
errors++;
}
}
if(data9.entry !== undefined){
if(typeof data9.entry !== "string"){
const err25 = {instancePath:instancePath+"/runtime/entry",schemaPath:"#/$defs/PublicRuntimeV1/properties/entry/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err25];
}
else {
vErrors.push(err25);
}
errors++;
}
}
}
else {
const err26 = {instancePath:instancePath+"/runtime",schemaPath:"#/$defs/PublicRuntimeV1/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err26];
}
else {
vErrors.push(err26);
}
errors++;
}
}
if(data.schemaVersion !== undefined){
let data11 = data.schemaVersion;
if(!(((typeof data11 == "number") && (!(data11 % 1) && !isNaN(data11))) && (isFinite(data11)))){
const err27 = {instancePath:instancePath+"/schemaVersion",schemaPath:"#/properties/schemaVersion/type",keyword:"type",params:{type: "integer"},message:"must be integer"};
if(vErrors === null){
vErrors = [err27];
}
else {
vErrors.push(err27);
}
errors++;
}
if((typeof data11 == "number") && (isFinite(data11))){
if(data11 > 4294967295 || isNaN(data11)){
const err28 = {instancePath:instancePath+"/schemaVersion",schemaPath:"#/properties/schemaVersion/maximum",keyword:"maximum",params:{comparison: "<=", limit: 4294967295},message:"must be <= 4294967295"};
if(vErrors === null){
vErrors = [err28];
}
else {
vErrors.push(err28);
}
errors++;
}
if(data11 < 0 || isNaN(data11)){
const err29 = {instancePath:instancePath+"/schemaVersion",schemaPath:"#/properties/schemaVersion/minimum",keyword:"minimum",params:{comparison: ">=", limit: 0},message:"must be >= 0"};
if(vErrors === null){
vErrors = [err29];
}
else {
vErrors.push(err29);
}
errors++;
}
}
}
if(data.settings !== undefined){
let data12 = data.settings;
if(Array.isArray(data12)){
const len1 = data12.length;
for(let i1=0; i1<len1; i1++){
if(!(validate25(data12[i1], {instancePath:instancePath+"/settings/" + i1,parentData:data12,parentDataProperty:i1,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate25.errors : vErrors.concat(validate25.errors);
errors = vErrors.length;
}
}
}
else {
const err30 = {instancePath:instancePath+"/settings",schemaPath:"#/properties/settings/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err30];
}
else {
vErrors.push(err30);
}
errors++;
}
}
if(data.supportedPlatforms !== undefined){
let data14 = data.supportedPlatforms;
if(Array.isArray(data14)){
const len2 = data14.length;
for(let i2=0; i2<len2; i2++){
let data15 = data14[i2];
if(typeof data15 !== "string"){
const err31 = {instancePath:instancePath+"/supportedPlatforms/" + i2,schemaPath:"#/$defs/PublicPlatform/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err31];
}
else {
vErrors.push(err31);
}
errors++;
}
if(!((data15 === "windows") || (data15 === "macos"))){
const err32 = {instancePath:instancePath+"/supportedPlatforms/" + i2,schemaPath:"#/$defs/PublicPlatform/enum",keyword:"enum",params:{allowedValues: schema41.enum},message:"must be equal to one of the allowed values"};
if(vErrors === null){
vErrors = [err32];
}
else {
vErrors.push(err32);
}
errors++;
}
}
}
else {
const err33 = {instancePath:instancePath+"/supportedPlatforms",schemaPath:"#/properties/supportedPlatforms/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err33];
}
else {
vErrors.push(err33);
}
errors++;
}
}
if(data.version !== undefined){
if(typeof data.version !== "string"){
const err34 = {instancePath:instancePath+"/version",schemaPath:"#/properties/version/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err34];
}
else {
vErrors.push(err34);
}
errors++;
}
}
if(data.window !== undefined){
let data17 = data.window;
const _errs42 = errors;
let valid12 = false;
const _errs43 = errors;
if(data17 && typeof data17 == "object" && !Array.isArray(data17)){
if(data17.entry === undefined){
const err35 = {instancePath:instancePath+"/window",schemaPath:"#/$defs/PublicWindowV1/required",keyword:"required",params:{missingProperty: "entry"},message:"must have required property '"+"entry"+"'"};
if(vErrors === null){
vErrors = [err35];
}
else {
vErrors.push(err35);
}
errors++;
}
for(const key2 in data17){
if(!(key2 === "entry")){
const err36 = {instancePath:instancePath+"/window",schemaPath:"#/$defs/PublicWindowV1/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key2},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err36];
}
else {
vErrors.push(err36);
}
errors++;
}
}
if(data17.entry !== undefined){
if(typeof data17.entry !== "string"){
const err37 = {instancePath:instancePath+"/window/entry",schemaPath:"#/$defs/PublicWindowV1/properties/entry/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err37];
}
else {
vErrors.push(err37);
}
errors++;
}
}
}
else {
const err38 = {instancePath:instancePath+"/window",schemaPath:"#/$defs/PublicWindowV1/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err38];
}
else {
vErrors.push(err38);
}
errors++;
}
var _valid1 = _errs43 === errors;
valid12 = valid12 || _valid1;
const _errs49 = errors;
if(data17 !== null){
const err39 = {instancePath:instancePath+"/window",schemaPath:"#/properties/window/anyOf/1/type",keyword:"type",params:{type: "null"},message:"must be null"};
if(vErrors === null){
vErrors = [err39];
}
else {
vErrors.push(err39);
}
errors++;
}
var _valid1 = _errs49 === errors;
valid12 = valid12 || _valid1;
if(!valid12){
const err40 = {instancePath:instancePath+"/window",schemaPath:"#/properties/window/anyOf",keyword:"anyOf",params:{},message:"must match a schema in anyOf"};
if(vErrors === null){
vErrors = [err40];
}
else {
vErrors.push(err40);
}
errors++;
}
else {
errors = _errs42;
if(vErrors !== null){
if(_errs42){
vErrors.length = _errs42;
}
else {
vErrors = null;
}
}
}
}
}
else {
const err41 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err41];
}
else {
vErrors.push(err41);
}
errors++;
}
validate20.errors = vErrors;
return errors === 0;
}
validate20.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

