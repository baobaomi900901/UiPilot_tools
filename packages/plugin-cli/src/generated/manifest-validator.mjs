"use strict";
export const validate = validate20;
export default validate20;
const schema31 = {"$defs":{"PanelHostKeyDeclaration":{"enum":["ArrowDown","ArrowUp","Primary+N","Tab","Shift+Tab","Enter","Delete"],"type":"string"},"PanelHostKeyFocus":{"enum":["content","host"],"type":"string"},"PluginNetworkCredential":{"additionalProperties":false,"properties":{"host":{"pattern":"^[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$","type":"string"},"id":{"pattern":"^[a-z][a-z0-9.-]{0,63}$","type":"string"},"settingKey":{"pattern":"^[a-z][a-z0-9.-]{0,63}$","type":"string"},"type":{"$ref":"#/$defs/PluginNetworkCredentialType"}},"required":["id","settingKey","host","type"],"type":"object"},"PluginNetworkCredentialType":{"enum":["bearer"],"type":"string"},"PublicActivationMode":{"enum":["live","submit"],"type":"string"},"PublicCommandV1":{"additionalProperties":false,"properties":{"activationMode":{"$ref":"#/$defs/PublicActivationMode"},"defaultName":{"type":"string"},"inputCapabilities":{"items":{"$ref":"#/$defs/PublicInputCapability"},"type":"array"},"inputPlaceholder":{"default":null,"type":["string","null"]},"inputRequired":{"type":"boolean"},"outputMode":{"$ref":"#/$defs/PublicOutputMode"},"summary":{"default":null,"type":["string","null"]}},"required":["defaultName","activationMode","outputMode","inputRequired"],"type":"object"},"PublicInputCapability":{"enum":["image.ocr"],"type":"string"},"PublicNamedCommandV1":{"additionalProperties":false,"properties":{"activationMode":{"$ref":"#/$defs/PublicActivationMode"},"defaultName":{"type":"string"},"id":{"pattern":"^[a-z][a-z0-9-]{0,31}$","type":"string"},"inputCapabilities":{"items":{"$ref":"#/$defs/PublicInputCapability"},"type":"array"},"inputPlaceholder":{"default":null,"type":["string","null"]},"inputRequired":{"type":"boolean"},"outputMode":{"const":"mainResult"},"summary":{"default":null,"type":["string","null"]}},"required":["id","defaultName","activationMode","outputMode","inputRequired"],"type":"object"},"PublicNetworkV1":{"additionalProperties":false,"properties":{"credentials":{"items":{"$ref":"#/$defs/PluginNetworkCredential"},"maxItems":8,"minItems":1,"type":"array"},"httpsHosts":{"items":{"pattern":"^[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$","type":"string"},"maxItems":8,"minItems":1,"type":"array","uniqueItems":true}},"required":["httpsHosts"],"type":"object"},"PublicOutputMode":{"enum":["mainResult","window","panel"],"type":"string"},"PublicPanelV1":{"additionalProperties":false,"properties":{"entry":{"type":"string"},"hostKeyFocus":{"$ref":"#/$defs/PanelHostKeyFocus"},"hostKeys":{"default":[],"items":{"enum":["ArrowDown","ArrowUp","Primary+N","Tab","Shift+Tab","Enter","Delete"],"type":"string"},"maxItems":8,"type":"array","uniqueItems":true},"runtimeActions":{"default":false,"type":"boolean"}},"required":["entry"],"type":"object"},"PublicPermission":{"enum":["ui.window","ui.panel","clipboard.write","speech.speak","clipboard.paste","files.openText","url.open","clipboard.read","selection.read","clipboard.history.read","clipboard.history.paste","clipboard.history.copy","network.https","credentials.youdaoTranslation","credentials.baiduOcr","credentials.deepseek","files.userSelected","files.index.readAll","notifications.publish","timer.control","background.schedule"],"type":"string"},"PublicPlatform":{"enum":["windows","macos"],"type":"string"},"PublicRuntimeV1":{"additionalProperties":false,"properties":{"entry":{"type":"string"},"resultActions":{"type":"boolean"}},"required":["entry"],"type":"object"},"PublicSelectOptionV1":{"additionalProperties":false,"properties":{"label":{"type":"string"},"value":{"type":"string"}},"required":["value","label"],"type":"object"},"PublicSettingV1":{"oneOf":[{"additionalProperties":false,"properties":{"default":{"default":null,"type":["string","null"]},"key":{"type":"string"},"label":{"type":"string"},"type":{"const":"text","type":"string"}},"required":["type","key","label"],"type":"object"},{"additionalProperties":false,"properties":{"key":{"type":"string"},"label":{"type":"string"},"type":{"const":"secret","type":"string"}},"required":["type","key","label"],"type":"object"},{"additionalProperties":false,"properties":{"default":{"default":null,"type":["number","null"]},"key":{"type":"string"},"label":{"type":"string"},"max":{"default":null,"type":["number","null"]},"min":{"default":null,"type":["number","null"]},"step":{"default":null,"type":["number","null"]},"type":{"const":"number","type":"string"}},"required":["type","key","label"],"type":"object"},{"additionalProperties":false,"properties":{"default":{"default":null,"type":["boolean","null"]},"key":{"type":"string"},"label":{"type":"string"},"type":{"const":"boolean","type":"string"}},"required":["type","key","label"],"type":"object"},{"additionalProperties":false,"properties":{"default":{"default":null,"type":["string","null"]},"key":{"type":"string"},"label":{"type":"string"},"options":{"items":{"$ref":"#/$defs/PublicSelectOptionV1"},"type":"array"},"type":{"const":"select","type":"string"}},"required":["type","key","label","options"],"type":"object"}]},"PublicShareReceiverV1":{"additionalProperties":false,"properties":{"commandId":{"const":"default"},"id":{"not":{"enum":["__proto__","prototype","constructor"]},"pattern":"^[a-z][a-z0-9.-]{0,63}$","type":"string"},"maxBytes":{"maximum":16384,"minimum":1,"type":"integer"},"title":{"pattern":"^[^\\u0000-\\u001f\\u007f-\\u009f\\ud800-\\udfff]*$","type":"string","allOf":[{"pattern":"^[\\s\\S]{1,80}(?![\\s\\S])"}]},"types":{"type":"array","minItems":1,"maxItems":1,"items":{"const":"text/plain"}}},"required":["id","title","commandId","types","maxBytes"],"type":"object"},"PublicShareV1":{"additionalProperties":false,"anyOf":[{"properties":{"source":true},"required":["source"]},{"properties":{"receivers":true},"required":["receivers"]}],"properties":{"receivers":{"items":{"$ref":"#/$defs/PublicShareReceiverV1"},"maxItems":8,"minItems":1,"type":"array"},"source":{"const":true},"version":{"maximum":1,"minimum":1,"type":"integer"}},"required":["version"],"type":"object"},"PublicWindowV1":{"additionalProperties":false,"properties":{"entry":{"type":"string"},"runtimeActions":{"default":false,"type":"boolean"}},"required":["entry"],"type":"object"}},"$schema":"https://json-schema.org/draft/2020-12/schema","additionalProperties":false,"oneOf":[{"not":{"properties":{"commands":true},"required":["commands"]},"properties":{"command":true},"required":["command"]},{"not":{"anyOf":[{"properties":{"command":true},"required":["command"]},{"properties":{"window":true},"required":["window"]},{"properties":{"panel":true},"required":["panel"]}]},"properties":{"commands":true,"permissions":{"not":{"contains":{"enum":["ui.window","ui.panel","speech.speak","timer.control","clipboard.history.read","clipboard.history.paste","clipboard.history.copy"]}},"type":"array"}},"required":["commands"]}],"properties":{"apiVersion":{"minimum":0,"type":"integer","maximum":4294967295},"command":{"$ref":"#/$defs/PublicCommandV1"},"commands":{"items":{"$ref":"#/$defs/PublicNamedCommandV1"},"maxItems":16,"minItems":1,"type":"array"},"description":{"default":null,"type":["string","null"]},"minimumHostVersion":{"type":"string"},"name":{"type":"string"},"network":{"$ref":"#/$defs/PublicNetworkV1"},"panel":{"anyOf":[{"$ref":"#/$defs/PublicPanelV1"},{"type":"null"}]},"permissions":{"items":{"$ref":"#/$defs/PublicPermission"},"type":"array"},"pluginId":{"type":"string"},"runtime":{"$ref":"#/$defs/PublicRuntimeV1"},"schemaVersion":{"minimum":0,"type":"integer","maximum":4294967295},"settings":{"default":[],"items":{"$ref":"#/$defs/PublicSettingV1"},"type":"array"},"share":{"$ref":"#/$defs/PublicShareV1"},"supportedPlatforms":{"items":{"$ref":"#/$defs/PublicPlatform"},"type":"array"},"version":{"type":"string"},"window":{"anyOf":[{"$ref":"#/$defs/PublicWindowV1"},{"type":"null"}]}},"required":["schemaVersion","pluginId","version","apiVersion","minimumHostVersion","name","supportedPlatforms","runtime","permissions"],"title":"PublicManifestV1","type":"object"};
const schema44 = {"enum":["ui.window","ui.panel","clipboard.write","speech.speak","clipboard.paste","files.openText","url.open","clipboard.read","selection.read","clipboard.history.read","clipboard.history.paste","clipboard.history.copy","network.https","credentials.youdaoTranslation","credentials.baiduOcr","credentials.deepseek","files.userSelected","files.index.readAll","notifications.publish","timer.control","background.schedule"],"type":"string"};
const schema45 = {"additionalProperties":false,"properties":{"entry":{"type":"string"},"resultActions":{"type":"boolean"}},"required":["entry"],"type":"object"};
const schema50 = {"enum":["windows","macos"],"type":"string"};
const schema51 = {"additionalProperties":false,"properties":{"entry":{"type":"string"},"runtimeActions":{"default":false,"type":"boolean"}},"required":["entry"],"type":"object"};
const func1 = Object.prototype.hasOwnProperty;
const schema32 = {"additionalProperties":false,"properties":{"activationMode":{"$ref":"#/$defs/PublicActivationMode"},"defaultName":{"type":"string"},"inputCapabilities":{"items":{"$ref":"#/$defs/PublicInputCapability"},"type":"array"},"inputPlaceholder":{"default":null,"type":["string","null"]},"inputRequired":{"type":"boolean"},"outputMode":{"$ref":"#/$defs/PublicOutputMode"},"summary":{"default":null,"type":["string","null"]}},"required":["defaultName","activationMode","outputMode","inputRequired"],"type":"object"};
const schema33 = {"enum":["live","submit"],"type":"string"};
const schema34 = {"enum":["image.ocr"],"type":"string"};
const schema35 = {"enum":["mainResult","window","panel"],"type":"string"};

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
if(!(((((((key0 === "activationMode") || (key0 === "defaultName")) || (key0 === "inputCapabilities")) || (key0 === "inputPlaceholder")) || (key0 === "inputRequired")) || (key0 === "outputMode")) || (key0 === "summary"))){
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
if(data.inputCapabilities !== undefined){
let data2 = data.inputCapabilities;
if(Array.isArray(data2)){
const len0 = data2.length;
for(let i0=0; i0<len0; i0++){
let data3 = data2[i0];
if(typeof data3 !== "string"){
const err8 = {instancePath:instancePath+"/inputCapabilities/" + i0,schemaPath:"#/$defs/PublicInputCapability/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
if(!(data3 === "image.ocr")){
const err9 = {instancePath:instancePath+"/inputCapabilities/" + i0,schemaPath:"#/$defs/PublicInputCapability/enum",keyword:"enum",params:{allowedValues: schema34.enum},message:"must be equal to one of the allowed values"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
}
}
else {
const err10 = {instancePath:instancePath+"/inputCapabilities",schemaPath:"#/properties/inputCapabilities/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
}
if(data.inputPlaceholder !== undefined){
let data4 = data.inputPlaceholder;
if((typeof data4 !== "string") && (data4 !== null)){
const err11 = {instancePath:instancePath+"/inputPlaceholder",schemaPath:"#/properties/inputPlaceholder/type",keyword:"type",params:{type: schema32.properties.inputPlaceholder.type},message:"must be string,null"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
}
if(data.inputRequired !== undefined){
if(typeof data.inputRequired !== "boolean"){
const err12 = {instancePath:instancePath+"/inputRequired",schemaPath:"#/properties/inputRequired/type",keyword:"type",params:{type: "boolean"},message:"must be boolean"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
}
if(data.outputMode !== undefined){
let data6 = data.outputMode;
if(typeof data6 !== "string"){
const err13 = {instancePath:instancePath+"/outputMode",schemaPath:"#/$defs/PublicOutputMode/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
if(!(((data6 === "mainResult") || (data6 === "window")) || (data6 === "panel"))){
const err14 = {instancePath:instancePath+"/outputMode",schemaPath:"#/$defs/PublicOutputMode/enum",keyword:"enum",params:{allowedValues: schema35.enum},message:"must be equal to one of the allowed values"};
if(vErrors === null){
vErrors = [err14];
}
else {
vErrors.push(err14);
}
errors++;
}
}
if(data.summary !== undefined){
let data7 = data.summary;
if((typeof data7 !== "string") && (data7 !== null)){
const err15 = {instancePath:instancePath+"/summary",schemaPath:"#/properties/summary/type",keyword:"type",params:{type: schema32.properties.summary.type},message:"must be string,null"};
if(vErrors === null){
vErrors = [err15];
}
else {
vErrors.push(err15);
}
errors++;
}
}
}
else {
const err16 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err16];
}
else {
vErrors.push(err16);
}
errors++;
}
validate21.errors = vErrors;
return errors === 0;
}
validate21.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

const schema36 = {"additionalProperties":false,"properties":{"activationMode":{"$ref":"#/$defs/PublicActivationMode"},"defaultName":{"type":"string"},"id":{"pattern":"^[a-z][a-z0-9-]{0,31}$","type":"string"},"inputCapabilities":{"items":{"$ref":"#/$defs/PublicInputCapability"},"type":"array"},"inputPlaceholder":{"default":null,"type":["string","null"]},"inputRequired":{"type":"boolean"},"outputMode":{"const":"mainResult"},"summary":{"default":null,"type":["string","null"]}},"required":["id","defaultName","activationMode","outputMode","inputRequired"],"type":"object"};
const pattern4 = new RegExp("^[a-z][a-z0-9-]{0,31}$", "u");

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
if(data.id === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "id"},message:"must have required property '"+"id"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.defaultName === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "defaultName"},message:"must have required property '"+"defaultName"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.activationMode === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "activationMode"},message:"must have required property '"+"activationMode"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
if(data.outputMode === undefined){
const err3 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "outputMode"},message:"must have required property '"+"outputMode"+"'"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
if(data.inputRequired === undefined){
const err4 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "inputRequired"},message:"must have required property '"+"inputRequired"+"'"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
for(const key0 in data){
if(!((((((((key0 === "activationMode") || (key0 === "defaultName")) || (key0 === "id")) || (key0 === "inputCapabilities")) || (key0 === "inputPlaceholder")) || (key0 === "inputRequired")) || (key0 === "outputMode")) || (key0 === "summary"))){
const err5 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
}
if(data.activationMode !== undefined){
let data0 = data.activationMode;
if(typeof data0 !== "string"){
const err6 = {instancePath:instancePath+"/activationMode",schemaPath:"#/$defs/PublicActivationMode/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
if(!((data0 === "live") || (data0 === "submit"))){
const err7 = {instancePath:instancePath+"/activationMode",schemaPath:"#/$defs/PublicActivationMode/enum",keyword:"enum",params:{allowedValues: schema33.enum},message:"must be equal to one of the allowed values"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
if(data.defaultName !== undefined){
if(typeof data.defaultName !== "string"){
const err8 = {instancePath:instancePath+"/defaultName",schemaPath:"#/properties/defaultName/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
if(data.id !== undefined){
let data2 = data.id;
if(typeof data2 === "string"){
if(!pattern4.test(data2)){
const err9 = {instancePath:instancePath+"/id",schemaPath:"#/properties/id/pattern",keyword:"pattern",params:{pattern: "^[a-z][a-z0-9-]{0,31}$"},message:"must match pattern \""+"^[a-z][a-z0-9-]{0,31}$"+"\""};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
}
else {
const err10 = {instancePath:instancePath+"/id",schemaPath:"#/properties/id/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
}
if(data.inputCapabilities !== undefined){
let data3 = data.inputCapabilities;
if(Array.isArray(data3)){
const len0 = data3.length;
for(let i0=0; i0<len0; i0++){
let data4 = data3[i0];
if(typeof data4 !== "string"){
const err11 = {instancePath:instancePath+"/inputCapabilities/" + i0,schemaPath:"#/$defs/PublicInputCapability/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
if(!(data4 === "image.ocr")){
const err12 = {instancePath:instancePath+"/inputCapabilities/" + i0,schemaPath:"#/$defs/PublicInputCapability/enum",keyword:"enum",params:{allowedValues: schema34.enum},message:"must be equal to one of the allowed values"};
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
const err13 = {instancePath:instancePath+"/inputCapabilities",schemaPath:"#/properties/inputCapabilities/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
}
if(data.inputPlaceholder !== undefined){
let data5 = data.inputPlaceholder;
if((typeof data5 !== "string") && (data5 !== null)){
const err14 = {instancePath:instancePath+"/inputPlaceholder",schemaPath:"#/properties/inputPlaceholder/type",keyword:"type",params:{type: schema36.properties.inputPlaceholder.type},message:"must be string,null"};
if(vErrors === null){
vErrors = [err14];
}
else {
vErrors.push(err14);
}
errors++;
}
}
if(data.inputRequired !== undefined){
if(typeof data.inputRequired !== "boolean"){
const err15 = {instancePath:instancePath+"/inputRequired",schemaPath:"#/properties/inputRequired/type",keyword:"type",params:{type: "boolean"},message:"must be boolean"};
if(vErrors === null){
vErrors = [err15];
}
else {
vErrors.push(err15);
}
errors++;
}
}
if(data.outputMode !== undefined){
if("mainResult" !== data.outputMode){
const err16 = {instancePath:instancePath+"/outputMode",schemaPath:"#/properties/outputMode/const",keyword:"const",params:{allowedValue: "mainResult"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err16];
}
else {
vErrors.push(err16);
}
errors++;
}
}
if(data.summary !== undefined){
let data8 = data.summary;
if((typeof data8 !== "string") && (data8 !== null)){
const err17 = {instancePath:instancePath+"/summary",schemaPath:"#/properties/summary/type",keyword:"type",params:{type: schema36.properties.summary.type},message:"must be string,null"};
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
const err18 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err18];
}
else {
vErrors.push(err18);
}
errors++;
}
validate23.errors = vErrors;
return errors === 0;
}
validate23.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

const schema39 = {"additionalProperties":false,"properties":{"credentials":{"items":{"$ref":"#/$defs/PluginNetworkCredential"},"maxItems":8,"minItems":1,"type":"array"},"httpsHosts":{"items":{"pattern":"^[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$","type":"string"},"maxItems":8,"minItems":1,"type":"array","uniqueItems":true}},"required":["httpsHosts"],"type":"object"};
const schema40 = {"additionalProperties":false,"properties":{"host":{"pattern":"^[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$","type":"string"},"id":{"pattern":"^[a-z][a-z0-9.-]{0,63}$","type":"string"},"settingKey":{"pattern":"^[a-z][a-z0-9.-]{0,63}$","type":"string"},"type":{"$ref":"#/$defs/PluginNetworkCredentialType"}},"required":["id","settingKey","host","type"],"type":"object"};
const schema41 = {"enum":["bearer"],"type":"string"};
const pattern5 = new RegExp("^[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$", "u");
const pattern6 = new RegExp("^[a-z][a-z0-9.-]{0,63}$", "u");

function validate26(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate26.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.id === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "id"},message:"must have required property '"+"id"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
if(data.settingKey === undefined){
const err1 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "settingKey"},message:"must have required property '"+"settingKey"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
if(data.host === undefined){
const err2 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "host"},message:"must have required property '"+"host"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
if(data.type === undefined){
const err3 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "type"},message:"must have required property '"+"type"+"'"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
for(const key0 in data){
if(!((((key0 === "host") || (key0 === "id")) || (key0 === "settingKey")) || (key0 === "type"))){
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
if(data.host !== undefined){
let data0 = data.host;
if(typeof data0 === "string"){
if(!pattern5.test(data0)){
const err5 = {instancePath:instancePath+"/host",schemaPath:"#/properties/host/pattern",keyword:"pattern",params:{pattern: "^[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$"},message:"must match pattern \""+"^[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$"+"\""};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
}
else {
const err6 = {instancePath:instancePath+"/host",schemaPath:"#/properties/host/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
}
if(data.id !== undefined){
let data1 = data.id;
if(typeof data1 === "string"){
if(!pattern6.test(data1)){
const err7 = {instancePath:instancePath+"/id",schemaPath:"#/properties/id/pattern",keyword:"pattern",params:{pattern: "^[a-z][a-z0-9.-]{0,63}$"},message:"must match pattern \""+"^[a-z][a-z0-9.-]{0,63}$"+"\""};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
else {
const err8 = {instancePath:instancePath+"/id",schemaPath:"#/properties/id/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
if(data.settingKey !== undefined){
let data2 = data.settingKey;
if(typeof data2 === "string"){
if(!pattern6.test(data2)){
const err9 = {instancePath:instancePath+"/settingKey",schemaPath:"#/properties/settingKey/pattern",keyword:"pattern",params:{pattern: "^[a-z][a-z0-9.-]{0,63}$"},message:"must match pattern \""+"^[a-z][a-z0-9.-]{0,63}$"+"\""};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
}
else {
const err10 = {instancePath:instancePath+"/settingKey",schemaPath:"#/properties/settingKey/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
}
if(data.type !== undefined){
let data3 = data.type;
if(typeof data3 !== "string"){
const err11 = {instancePath:instancePath+"/type",schemaPath:"#/$defs/PluginNetworkCredentialType/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
if(!(data3 === "bearer")){
const err12 = {instancePath:instancePath+"/type",schemaPath:"#/$defs/PluginNetworkCredentialType/enum",keyword:"enum",params:{allowedValues: schema41.enum},message:"must be equal to one of the allowed values"};
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
validate26.errors = vErrors;
return errors === 0;
}
validate26.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};


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
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.httpsHosts === undefined){
const err0 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "httpsHosts"},message:"must have required property '"+"httpsHosts"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
for(const key0 in data){
if(!((key0 === "credentials") || (key0 === "httpsHosts"))){
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
if(data.credentials !== undefined){
let data0 = data.credentials;
if(Array.isArray(data0)){
if(data0.length > 8){
const err2 = {instancePath:instancePath+"/credentials",schemaPath:"#/properties/credentials/maxItems",keyword:"maxItems",params:{limit: 8},message:"must NOT have more than 8 items"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
if(data0.length < 1){
const err3 = {instancePath:instancePath+"/credentials",schemaPath:"#/properties/credentials/minItems",keyword:"minItems",params:{limit: 1},message:"must NOT have fewer than 1 items"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
const len0 = data0.length;
for(let i0=0; i0<len0; i0++){
if(!(validate26(data0[i0], {instancePath:instancePath+"/credentials/" + i0,parentData:data0,parentDataProperty:i0,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate26.errors : vErrors.concat(validate26.errors);
errors = vErrors.length;
}
}
}
else {
const err4 = {instancePath:instancePath+"/credentials",schemaPath:"#/properties/credentials/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
}
if(data.httpsHosts !== undefined){
let data2 = data.httpsHosts;
if(Array.isArray(data2)){
if(data2.length > 8){
const err5 = {instancePath:instancePath+"/httpsHosts",schemaPath:"#/properties/httpsHosts/maxItems",keyword:"maxItems",params:{limit: 8},message:"must NOT have more than 8 items"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
if(data2.length < 1){
const err6 = {instancePath:instancePath+"/httpsHosts",schemaPath:"#/properties/httpsHosts/minItems",keyword:"minItems",params:{limit: 1},message:"must NOT have fewer than 1 items"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
const len1 = data2.length;
for(let i1=0; i1<len1; i1++){
let data3 = data2[i1];
if(typeof data3 === "string"){
if(!pattern5.test(data3)){
const err7 = {instancePath:instancePath+"/httpsHosts/" + i1,schemaPath:"#/properties/httpsHosts/items/pattern",keyword:"pattern",params:{pattern: "^[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$"},message:"must match pattern \""+"^[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$"+"\""};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
else {
const err8 = {instancePath:instancePath+"/httpsHosts/" + i1,schemaPath:"#/properties/httpsHosts/items/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
}
let i2 = data2.length;
let j0;
if(i2 > 1){
const indices0 = {};
for(;i2--;){
let item0 = data2[i2];
if(typeof item0 !== "string"){
continue;
}
if(typeof indices0[item0] == "number"){
j0 = indices0[item0];
const err9 = {instancePath:instancePath+"/httpsHosts",schemaPath:"#/properties/httpsHosts/uniqueItems",keyword:"uniqueItems",params:{i: i2, j: j0},message:"must NOT have duplicate items (items ## "+j0+" and "+i2+" are identical)"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
break;
}
indices0[item0] = i2;
}
}
}
else {
const err10 = {instancePath:instancePath+"/httpsHosts",schemaPath:"#/properties/httpsHosts/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
}
}
else {
const err11 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
validate25.errors = vErrors;
return errors === 0;
}
validate25.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

const schema42 = {"additionalProperties":false,"properties":{"entry":{"type":"string"},"hostKeyFocus":{"$ref":"#/$defs/PanelHostKeyFocus"},"hostKeys":{"default":[],"items":{"enum":["ArrowDown","ArrowUp","Primary+N","Tab","Shift+Tab","Enter","Delete"],"type":"string"},"maxItems":8,"type":"array","uniqueItems":true},"runtimeActions":{"default":false,"type":"boolean"}},"required":["entry"],"type":"object"};
const schema43 = {"enum":["content","host"],"type":"string"};

function validate29(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate29.evaluated;
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
if(!((((key0 === "entry") || (key0 === "hostKeyFocus")) || (key0 === "hostKeys")) || (key0 === "runtimeActions"))){
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
if(data.hostKeyFocus !== undefined){
let data1 = data.hostKeyFocus;
if(typeof data1 !== "string"){
const err3 = {instancePath:instancePath+"/hostKeyFocus",schemaPath:"#/$defs/PanelHostKeyFocus/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
if(!((data1 === "content") || (data1 === "host"))){
const err4 = {instancePath:instancePath+"/hostKeyFocus",schemaPath:"#/$defs/PanelHostKeyFocus/enum",keyword:"enum",params:{allowedValues: schema43.enum},message:"must be equal to one of the allowed values"};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
}
if(data.hostKeys !== undefined){
let data2 = data.hostKeys;
if(Array.isArray(data2)){
if(data2.length > 8){
const err5 = {instancePath:instancePath+"/hostKeys",schemaPath:"#/properties/hostKeys/maxItems",keyword:"maxItems",params:{limit: 8},message:"must NOT have more than 8 items"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
const len0 = data2.length;
for(let i0=0; i0<len0; i0++){
let data3 = data2[i0];
if(typeof data3 !== "string"){
const err6 = {instancePath:instancePath+"/hostKeys/" + i0,schemaPath:"#/properties/hostKeys/items/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
if(!(((((((data3 === "ArrowDown") || (data3 === "ArrowUp")) || (data3 === "Primary+N")) || (data3 === "Tab")) || (data3 === "Shift+Tab")) || (data3 === "Enter")) || (data3 === "Delete"))){
const err7 = {instancePath:instancePath+"/hostKeys/" + i0,schemaPath:"#/properties/hostKeys/items/enum",keyword:"enum",params:{allowedValues: schema42.properties.hostKeys.items.enum},message:"must be equal to one of the allowed values"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
}
let i1 = data2.length;
let j0;
if(i1 > 1){
const indices0 = {};
for(;i1--;){
let item0 = data2[i1];
if(typeof item0 !== "string"){
continue;
}
if(typeof indices0[item0] == "number"){
j0 = indices0[item0];
const err8 = {instancePath:instancePath+"/hostKeys",schemaPath:"#/properties/hostKeys/uniqueItems",keyword:"uniqueItems",params:{i: i1, j: j0},message:"must NOT have duplicate items (items ## "+j0+" and "+i1+" are identical)"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
break;
}
indices0[item0] = i1;
}
}
}
else {
const err9 = {instancePath:instancePath+"/hostKeys",schemaPath:"#/properties/hostKeys/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
}
if(data.runtimeActions !== undefined){
if(typeof data.runtimeActions !== "boolean"){
const err10 = {instancePath:instancePath+"/runtimeActions",schemaPath:"#/properties/runtimeActions/type",keyword:"type",params:{type: "boolean"},message:"must be boolean"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
}
}
else {
const err11 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
validate29.errors = vErrors;
return errors === 0;
}
validate29.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

const schema46 = {"oneOf":[{"additionalProperties":false,"properties":{"default":{"default":null,"type":["string","null"]},"key":{"type":"string"},"label":{"type":"string"},"type":{"const":"text","type":"string"}},"required":["type","key","label"],"type":"object"},{"additionalProperties":false,"properties":{"key":{"type":"string"},"label":{"type":"string"},"type":{"const":"secret","type":"string"}},"required":["type","key","label"],"type":"object"},{"additionalProperties":false,"properties":{"default":{"default":null,"type":["number","null"]},"key":{"type":"string"},"label":{"type":"string"},"max":{"default":null,"type":["number","null"]},"min":{"default":null,"type":["number","null"]},"step":{"default":null,"type":["number","null"]},"type":{"const":"number","type":"string"}},"required":["type","key","label"],"type":"object"},{"additionalProperties":false,"properties":{"default":{"default":null,"type":["boolean","null"]},"key":{"type":"string"},"label":{"type":"string"},"type":{"const":"boolean","type":"string"}},"required":["type","key","label"],"type":"object"},{"additionalProperties":false,"properties":{"default":{"default":null,"type":["string","null"]},"key":{"type":"string"},"label":{"type":"string"},"options":{"items":{"$ref":"#/$defs/PublicSelectOptionV1"},"type":"array"},"type":{"const":"select","type":"string"}},"required":["type","key","label","options"],"type":"object"}]};
const schema47 = {"additionalProperties":false,"properties":{"label":{"type":"string"},"value":{"type":"string"}},"required":["value","label"],"type":"object"};

function validate31(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate31.evaluated;
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
const err4 = {instancePath:instancePath+"/default",schemaPath:"#/oneOf/0/properties/default/type",keyword:"type",params:{type: schema46.oneOf[0].properties.default.type},message:"must be string,null"};
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
const err23 = {instancePath:instancePath+"/default",schemaPath:"#/oneOf/2/properties/default/type",keyword:"type",params:{type: schema46.oneOf[2].properties.default.type},message:"must be number,null"};
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
const err26 = {instancePath:instancePath+"/max",schemaPath:"#/oneOf/2/properties/max/type",keyword:"type",params:{type: schema46.oneOf[2].properties.max.type},message:"must be number,null"};
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
const err27 = {instancePath:instancePath+"/min",schemaPath:"#/oneOf/2/properties/min/type",keyword:"type",params:{type: schema46.oneOf[2].properties.min.type},message:"must be number,null"};
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
const err28 = {instancePath:instancePath+"/step",schemaPath:"#/oneOf/2/properties/step/type",keyword:"type",params:{type: schema46.oneOf[2].properties.step.type},message:"must be number,null"};
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
const err36 = {instancePath:instancePath+"/default",schemaPath:"#/oneOf/3/properties/default/type",keyword:"type",params:{type: schema46.oneOf[3].properties.default.type},message:"must be boolean,null"};
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
const err47 = {instancePath:instancePath+"/default",schemaPath:"#/oneOf/4/properties/default/type",keyword:"type",params:{type: schema46.oneOf[4].properties.default.type},message:"must be string,null"};
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
validate31.errors = vErrors;
evaluated0.props = props0;
return errors === 0;
}
validate31.evaluated = {"dynamicProps":true,"dynamicItems":false};

const schema48 = {"additionalProperties":false,"anyOf":[{"properties":{"source":true},"required":["source"]},{"properties":{"receivers":true},"required":["receivers"]}],"properties":{"receivers":{"items":{"$ref":"#/$defs/PublicShareReceiverV1"},"maxItems":8,"minItems":1,"type":"array"},"source":{"const":true},"version":{"maximum":1,"minimum":1,"type":"integer"}},"required":["version"],"type":"object"};
const schema49 = {"additionalProperties":false,"properties":{"commandId":{"const":"default"},"id":{"not":{"enum":["__proto__","prototype","constructor"]},"pattern":"^[a-z][a-z0-9.-]{0,63}$","type":"string"},"maxBytes":{"maximum":16384,"minimum":1,"type":"integer"},"title":{"pattern":"^[^\\u0000-\\u001f\\u007f-\\u009f\\ud800-\\udfff]*$","type":"string","allOf":[{"pattern":"^[\\s\\S]{1,80}(?![\\s\\S])"}]},"types":{"type":"array","minItems":1,"maxItems":1,"items":{"const":"text/plain"}}},"required":["id","title","commandId","types","maxBytes"],"type":"object"};
const pattern10 = new RegExp("^[\\s\\S]{1,80}(?![\\s\\S])", "u");
const pattern11 = new RegExp("^[^\\u0000-\\u001f\\u007f-\\u009f\\ud800-\\udfff]*$", "u");

function validate33(data, {instancePath="", parentData, parentDataProperty, rootData=data, dynamicAnchors={}}={}){
let vErrors = null;
let errors = 0;
const evaluated0 = validate33.evaluated;
if(evaluated0.dynamicProps){
evaluated0.props = undefined;
}
if(evaluated0.dynamicItems){
evaluated0.items = undefined;
}
const _errs1 = errors;
let valid0 = false;
const _errs2 = errors;
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.source === undefined){
const err0 = {instancePath,schemaPath:"#/anyOf/0/required",keyword:"required",params:{missingProperty: "source"},message:"must have required property '"+"source"+"'"};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
}
var _valid0 = _errs2 === errors;
valid0 = valid0 || _valid0;
if(_valid0){
var props0 = {};
props0.source = true;
}
const _errs3 = errors;
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.receivers === undefined){
const err1 = {instancePath,schemaPath:"#/anyOf/1/required",keyword:"required",params:{missingProperty: "receivers"},message:"must have required property '"+"receivers"+"'"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
}
var _valid0 = _errs3 === errors;
valid0 = valid0 || _valid0;
if(_valid0){
if(props0 !== true){
props0 = props0 || {};
props0.receivers = true;
}
}
if(!valid0){
const err2 = {instancePath,schemaPath:"#/anyOf",keyword:"anyOf",params:{},message:"must match a schema in anyOf"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
else {
errors = _errs1;
if(vErrors !== null){
if(_errs1){
vErrors.length = _errs1;
}
else {
vErrors = null;
}
}
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.version === undefined){
const err3 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "version"},message:"must have required property '"+"version"+"'"};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
for(const key0 in data){
if(!(((key0 === "receivers") || (key0 === "source")) || (key0 === "version"))){
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
if(data.receivers !== undefined){
let data0 = data.receivers;
if(Array.isArray(data0)){
if(data0.length > 8){
const err5 = {instancePath:instancePath+"/receivers",schemaPath:"#/properties/receivers/maxItems",keyword:"maxItems",params:{limit: 8},message:"must NOT have more than 8 items"};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
if(data0.length < 1){
const err6 = {instancePath:instancePath+"/receivers",schemaPath:"#/properties/receivers/minItems",keyword:"minItems",params:{limit: 1},message:"must NOT have fewer than 1 items"};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
const len0 = data0.length;
for(let i0=0; i0<len0; i0++){
let data1 = data0[i0];
if(data1 && typeof data1 == "object" && !Array.isArray(data1)){
if(data1.id === undefined){
const err7 = {instancePath:instancePath+"/receivers/" + i0,schemaPath:"#/$defs/PublicShareReceiverV1/required",keyword:"required",params:{missingProperty: "id"},message:"must have required property '"+"id"+"'"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
if(data1.title === undefined){
const err8 = {instancePath:instancePath+"/receivers/" + i0,schemaPath:"#/$defs/PublicShareReceiverV1/required",keyword:"required",params:{missingProperty: "title"},message:"must have required property '"+"title"+"'"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
if(data1.commandId === undefined){
const err9 = {instancePath:instancePath+"/receivers/" + i0,schemaPath:"#/$defs/PublicShareReceiverV1/required",keyword:"required",params:{missingProperty: "commandId"},message:"must have required property '"+"commandId"+"'"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
if(data1.types === undefined){
const err10 = {instancePath:instancePath+"/receivers/" + i0,schemaPath:"#/$defs/PublicShareReceiverV1/required",keyword:"required",params:{missingProperty: "types"},message:"must have required property '"+"types"+"'"};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
if(data1.maxBytes === undefined){
const err11 = {instancePath:instancePath+"/receivers/" + i0,schemaPath:"#/$defs/PublicShareReceiverV1/required",keyword:"required",params:{missingProperty: "maxBytes"},message:"must have required property '"+"maxBytes"+"'"};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
for(const key1 in data1){
if(!(((((key1 === "commandId") || (key1 === "id")) || (key1 === "maxBytes")) || (key1 === "title")) || (key1 === "types"))){
const err12 = {instancePath:instancePath+"/receivers/" + i0,schemaPath:"#/$defs/PublicShareReceiverV1/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key1},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
}
if(data1.commandId !== undefined){
if("default" !== data1.commandId){
const err13 = {instancePath:instancePath+"/receivers/" + i0+"/commandId",schemaPath:"#/$defs/PublicShareReceiverV1/properties/commandId/const",keyword:"const",params:{allowedValue: "default"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
}
if(data1.id !== undefined){
let data3 = data1.id;
const _errs14 = errors;
const _errs15 = errors;
if(!(((data3 === "__proto__") || (data3 === "prototype")) || (data3 === "constructor"))){
const err14 = {};
if(vErrors === null){
vErrors = [err14];
}
else {
vErrors.push(err14);
}
errors++;
}
var valid6 = _errs15 === errors;
if(valid6){
const err15 = {instancePath:instancePath+"/receivers/" + i0+"/id",schemaPath:"#/$defs/PublicShareReceiverV1/properties/id/not",keyword:"not",params:{},message:"must NOT be valid"};
if(vErrors === null){
vErrors = [err15];
}
else {
vErrors.push(err15);
}
errors++;
}
else {
errors = _errs14;
if(vErrors !== null){
if(_errs14){
vErrors.length = _errs14;
}
else {
vErrors = null;
}
}
}
if(typeof data3 === "string"){
if(!pattern6.test(data3)){
const err16 = {instancePath:instancePath+"/receivers/" + i0+"/id",schemaPath:"#/$defs/PublicShareReceiverV1/properties/id/pattern",keyword:"pattern",params:{pattern: "^[a-z][a-z0-9.-]{0,63}$"},message:"must match pattern \""+"^[a-z][a-z0-9.-]{0,63}$"+"\""};
if(vErrors === null){
vErrors = [err16];
}
else {
vErrors.push(err16);
}
errors++;
}
}
else {
const err17 = {instancePath:instancePath+"/receivers/" + i0+"/id",schemaPath:"#/$defs/PublicShareReceiverV1/properties/id/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err17];
}
else {
vErrors.push(err17);
}
errors++;
}
}
if(data1.maxBytes !== undefined){
let data4 = data1.maxBytes;
if(!(((typeof data4 == "number") && (!(data4 % 1) && !isNaN(data4))) && (isFinite(data4)))){
const err18 = {instancePath:instancePath+"/receivers/" + i0+"/maxBytes",schemaPath:"#/$defs/PublicShareReceiverV1/properties/maxBytes/type",keyword:"type",params:{type: "integer"},message:"must be integer"};
if(vErrors === null){
vErrors = [err18];
}
else {
vErrors.push(err18);
}
errors++;
}
if((typeof data4 == "number") && (isFinite(data4))){
if(data4 > 16384 || isNaN(data4)){
const err19 = {instancePath:instancePath+"/receivers/" + i0+"/maxBytes",schemaPath:"#/$defs/PublicShareReceiverV1/properties/maxBytes/maximum",keyword:"maximum",params:{comparison: "<=", limit: 16384},message:"must be <= 16384"};
if(vErrors === null){
vErrors = [err19];
}
else {
vErrors.push(err19);
}
errors++;
}
if(data4 < 1 || isNaN(data4)){
const err20 = {instancePath:instancePath+"/receivers/" + i0+"/maxBytes",schemaPath:"#/$defs/PublicShareReceiverV1/properties/maxBytes/minimum",keyword:"minimum",params:{comparison: ">=", limit: 1},message:"must be >= 1"};
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
if(data1.title !== undefined){
let data5 = data1.title;
if(typeof data5 === "string"){
if(!pattern10.test(data5)){
const err21 = {instancePath:instancePath+"/receivers/" + i0+"/title",schemaPath:"#/$defs/PublicShareReceiverV1/properties/title/allOf/0/pattern",keyword:"pattern",params:{pattern: "^[\\s\\S]{1,80}(?![\\s\\S])"},message:"must match pattern \""+"^[\\s\\S]{1,80}(?![\\s\\S])"+"\""};
if(vErrors === null){
vErrors = [err21];
}
else {
vErrors.push(err21);
}
errors++;
}
}
if(typeof data5 === "string"){
if(!pattern11.test(data5)){
const err22 = {instancePath:instancePath+"/receivers/" + i0+"/title",schemaPath:"#/$defs/PublicShareReceiverV1/properties/title/pattern",keyword:"pattern",params:{pattern: "^[^\\u0000-\\u001f\\u007f-\\u009f\\ud800-\\udfff]*$"},message:"must match pattern \""+"^[^\\u0000-\\u001f\\u007f-\\u009f\\ud800-\\udfff]*$"+"\""};
if(vErrors === null){
vErrors = [err22];
}
else {
vErrors.push(err22);
}
errors++;
}
}
else {
const err23 = {instancePath:instancePath+"/receivers/" + i0+"/title",schemaPath:"#/$defs/PublicShareReceiverV1/properties/title/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err23];
}
else {
vErrors.push(err23);
}
errors++;
}
}
if(data1.types !== undefined){
let data6 = data1.types;
if(Array.isArray(data6)){
if(data6.length > 1){
const err24 = {instancePath:instancePath+"/receivers/" + i0+"/types",schemaPath:"#/$defs/PublicShareReceiverV1/properties/types/maxItems",keyword:"maxItems",params:{limit: 1},message:"must NOT have more than 1 items"};
if(vErrors === null){
vErrors = [err24];
}
else {
vErrors.push(err24);
}
errors++;
}
if(data6.length < 1){
const err25 = {instancePath:instancePath+"/receivers/" + i0+"/types",schemaPath:"#/$defs/PublicShareReceiverV1/properties/types/minItems",keyword:"minItems",params:{limit: 1},message:"must NOT have fewer than 1 items"};
if(vErrors === null){
vErrors = [err25];
}
else {
vErrors.push(err25);
}
errors++;
}
const len1 = data6.length;
for(let i1=0; i1<len1; i1++){
if("text/plain" !== data6[i1]){
const err26 = {instancePath:instancePath+"/receivers/" + i0+"/types/" + i1,schemaPath:"#/$defs/PublicShareReceiverV1/properties/types/items/const",keyword:"const",params:{allowedValue: "text/plain"},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err26];
}
else {
vErrors.push(err26);
}
errors++;
}
}
}
else {
const err27 = {instancePath:instancePath+"/receivers/" + i0+"/types",schemaPath:"#/$defs/PublicShareReceiverV1/properties/types/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err27];
}
else {
vErrors.push(err27);
}
errors++;
}
}
}
else {
const err28 = {instancePath:instancePath+"/receivers/" + i0,schemaPath:"#/$defs/PublicShareReceiverV1/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err28];
}
else {
vErrors.push(err28);
}
errors++;
}
}
}
else {
const err29 = {instancePath:instancePath+"/receivers",schemaPath:"#/properties/receivers/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err29];
}
else {
vErrors.push(err29);
}
errors++;
}
}
if(data.source !== undefined){
if(true !== data.source){
const err30 = {instancePath:instancePath+"/source",schemaPath:"#/properties/source/const",keyword:"const",params:{allowedValue: true},message:"must be equal to constant"};
if(vErrors === null){
vErrors = [err30];
}
else {
vErrors.push(err30);
}
errors++;
}
}
if(data.version !== undefined){
let data9 = data.version;
if(!(((typeof data9 == "number") && (!(data9 % 1) && !isNaN(data9))) && (isFinite(data9)))){
const err31 = {instancePath:instancePath+"/version",schemaPath:"#/properties/version/type",keyword:"type",params:{type: "integer"},message:"must be integer"};
if(vErrors === null){
vErrors = [err31];
}
else {
vErrors.push(err31);
}
errors++;
}
if((typeof data9 == "number") && (isFinite(data9))){
if(data9 > 1 || isNaN(data9)){
const err32 = {instancePath:instancePath+"/version",schemaPath:"#/properties/version/maximum",keyword:"maximum",params:{comparison: "<=", limit: 1},message:"must be <= 1"};
if(vErrors === null){
vErrors = [err32];
}
else {
vErrors.push(err32);
}
errors++;
}
if(data9 < 1 || isNaN(data9)){
const err33 = {instancePath:instancePath+"/version",schemaPath:"#/properties/version/minimum",keyword:"minimum",params:{comparison: ">=", limit: 1},message:"must be >= 1"};
if(vErrors === null){
vErrors = [err33];
}
else {
vErrors.push(err33);
}
errors++;
}
}
}
}
else {
const err34 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err34];
}
else {
vErrors.push(err34);
}
errors++;
}
validate33.errors = vErrors;
return errors === 0;
}
validate33.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};


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
const _errs1 = errors;
let valid0 = false;
let passing0 = null;
const _errs2 = errors;
const _errs3 = errors;
const _errs4 = errors;
if(data && typeof data == "object" && !Array.isArray(data)){
let missing0;
if((data.commands === undefined) && (missing0 = "commands")){
const err0 = {};
if(vErrors === null){
vErrors = [err0];
}
else {
vErrors.push(err0);
}
errors++;
}
}
var valid1 = _errs4 === errors;
if(valid1){
const err1 = {instancePath,schemaPath:"#/oneOf/0/not",keyword:"not",params:{},message:"must NOT be valid"};
if(vErrors === null){
vErrors = [err1];
}
else {
vErrors.push(err1);
}
errors++;
}
else {
errors = _errs3;
if(vErrors !== null){
if(_errs3){
vErrors.length = _errs3;
}
else {
vErrors = null;
}
}
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.command === undefined){
const err2 = {instancePath,schemaPath:"#/oneOf/0/required",keyword:"required",params:{missingProperty: "command"},message:"must have required property '"+"command"+"'"};
if(vErrors === null){
vErrors = [err2];
}
else {
vErrors.push(err2);
}
errors++;
}
}
var _valid0 = _errs2 === errors;
if(_valid0){
valid0 = true;
passing0 = 0;
var props0 = {};
props0.command = true;
}
const _errs5 = errors;
const _errs6 = errors;
const _errs7 = errors;
const _errs8 = errors;
let valid3 = false;
const _errs9 = errors;
if(data && typeof data == "object" && !Array.isArray(data)){
let missing1;
if((data.command === undefined) && (missing1 = "command")){
const err3 = {};
if(vErrors === null){
vErrors = [err3];
}
else {
vErrors.push(err3);
}
errors++;
}
}
var _valid1 = _errs9 === errors;
valid3 = valid3 || _valid1;
if(_valid1){
var props1 = {};
props1.command = true;
}
const _errs10 = errors;
if(data && typeof data == "object" && !Array.isArray(data)){
let missing2;
if((data.window === undefined) && (missing2 = "window")){
const err4 = {};
if(vErrors === null){
vErrors = [err4];
}
else {
vErrors.push(err4);
}
errors++;
}
}
var _valid1 = _errs10 === errors;
valid3 = valid3 || _valid1;
if(_valid1){
if(props1 !== true){
props1 = props1 || {};
props1.window = true;
}
}
const _errs11 = errors;
if(data && typeof data == "object" && !Array.isArray(data)){
let missing3;
if((data.panel === undefined) && (missing3 = "panel")){
const err5 = {};
if(vErrors === null){
vErrors = [err5];
}
else {
vErrors.push(err5);
}
errors++;
}
}
var _valid1 = _errs11 === errors;
valid3 = valid3 || _valid1;
if(_valid1){
if(props1 !== true){
props1 = props1 || {};
props1.panel = true;
}
}
if(!valid3){
const err6 = {};
if(vErrors === null){
vErrors = [err6];
}
else {
vErrors.push(err6);
}
errors++;
}
else {
errors = _errs8;
if(vErrors !== null){
if(_errs8){
vErrors.length = _errs8;
}
else {
vErrors = null;
}
}
}
var valid2 = _errs7 === errors;
if(valid2){
const err7 = {instancePath,schemaPath:"#/oneOf/1/not",keyword:"not",params:{},message:"must NOT be valid"};
if(vErrors === null){
vErrors = [err7];
}
else {
vErrors.push(err7);
}
errors++;
}
else {
errors = _errs6;
if(vErrors !== null){
if(_errs6){
vErrors.length = _errs6;
}
else {
vErrors = null;
}
}
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.commands === undefined){
const err8 = {instancePath,schemaPath:"#/oneOf/1/required",keyword:"required",params:{missingProperty: "commands"},message:"must have required property '"+"commands"+"'"};
if(vErrors === null){
vErrors = [err8];
}
else {
vErrors.push(err8);
}
errors++;
}
if(data.permissions !== undefined){
let data0 = data.permissions;
if(!(Array.isArray(data0))){
const err9 = {instancePath:instancePath+"/permissions",schemaPath:"#/oneOf/1/properties/permissions/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err9];
}
else {
vErrors.push(err9);
}
errors++;
}
const _errs14 = errors;
const _errs15 = errors;
if(Array.isArray(data0)){
const _errs16 = errors;
const len0 = data0.length;
for(let i0=0; i0<len0; i0++){
let data1 = data0[i0];
const _errs17 = errors;
if(!(((((((data1 === "ui.window") || (data1 === "ui.panel")) || (data1 === "speech.speak")) || (data1 === "timer.control")) || (data1 === "clipboard.history.read")) || (data1 === "clipboard.history.paste")) || (data1 === "clipboard.history.copy"))){
const err10 = {};
if(vErrors === null){
vErrors = [err10];
}
else {
vErrors.push(err10);
}
errors++;
}
var valid6 = _errs17 === errors;
if(valid6){
break;
}
}
if(!valid6){
const err11 = {};
if(vErrors === null){
vErrors = [err11];
}
else {
vErrors.push(err11);
}
errors++;
}
else {
errors = _errs16;
if(vErrors !== null){
if(_errs16){
vErrors.length = _errs16;
}
else {
vErrors = null;
}
}
}
}
var valid5 = _errs15 === errors;
if(valid5){
const err12 = {instancePath:instancePath+"/permissions",schemaPath:"#/oneOf/1/properties/permissions/not",keyword:"not",params:{},message:"must NOT be valid"};
if(vErrors === null){
vErrors = [err12];
}
else {
vErrors.push(err12);
}
errors++;
}
else {
errors = _errs14;
if(vErrors !== null){
if(_errs14){
vErrors.length = _errs14;
}
else {
vErrors = null;
}
}
}
}
}
var _valid0 = _errs5 === errors;
if(_valid0 && valid0){
valid0 = false;
passing0 = [passing0, 1];
}
else {
if(_valid0){
valid0 = true;
passing0 = 1;
if(props0 !== true){
props0 = props0 || {};
props0.commands = true;
props0.permissions = true;
}
}
}
if(!valid0){
const err13 = {instancePath,schemaPath:"#/oneOf",keyword:"oneOf",params:{passingSchemas: passing0},message:"must match exactly one schema in oneOf"};
if(vErrors === null){
vErrors = [err13];
}
else {
vErrors.push(err13);
}
errors++;
}
else {
errors = _errs1;
if(vErrors !== null){
if(_errs1){
vErrors.length = _errs1;
}
else {
vErrors = null;
}
}
}
if(data && typeof data == "object" && !Array.isArray(data)){
if(data.schemaVersion === undefined){
const err14 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "schemaVersion"},message:"must have required property '"+"schemaVersion"+"'"};
if(vErrors === null){
vErrors = [err14];
}
else {
vErrors.push(err14);
}
errors++;
}
if(data.pluginId === undefined){
const err15 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "pluginId"},message:"must have required property '"+"pluginId"+"'"};
if(vErrors === null){
vErrors = [err15];
}
else {
vErrors.push(err15);
}
errors++;
}
if(data.version === undefined){
const err16 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "version"},message:"must have required property '"+"version"+"'"};
if(vErrors === null){
vErrors = [err16];
}
else {
vErrors.push(err16);
}
errors++;
}
if(data.apiVersion === undefined){
const err17 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "apiVersion"},message:"must have required property '"+"apiVersion"+"'"};
if(vErrors === null){
vErrors = [err17];
}
else {
vErrors.push(err17);
}
errors++;
}
if(data.minimumHostVersion === undefined){
const err18 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "minimumHostVersion"},message:"must have required property '"+"minimumHostVersion"+"'"};
if(vErrors === null){
vErrors = [err18];
}
else {
vErrors.push(err18);
}
errors++;
}
if(data.name === undefined){
const err19 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "name"},message:"must have required property '"+"name"+"'"};
if(vErrors === null){
vErrors = [err19];
}
else {
vErrors.push(err19);
}
errors++;
}
if(data.supportedPlatforms === undefined){
const err20 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "supportedPlatforms"},message:"must have required property '"+"supportedPlatforms"+"'"};
if(vErrors === null){
vErrors = [err20];
}
else {
vErrors.push(err20);
}
errors++;
}
if(data.runtime === undefined){
const err21 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "runtime"},message:"must have required property '"+"runtime"+"'"};
if(vErrors === null){
vErrors = [err21];
}
else {
vErrors.push(err21);
}
errors++;
}
if(data.permissions === undefined){
const err22 = {instancePath,schemaPath:"#/required",keyword:"required",params:{missingProperty: "permissions"},message:"must have required property '"+"permissions"+"'"};
if(vErrors === null){
vErrors = [err22];
}
else {
vErrors.push(err22);
}
errors++;
}
for(const key0 in data){
if(!(func1.call(schema31.properties, key0))){
const err23 = {instancePath,schemaPath:"#/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key0},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err23];
}
else {
vErrors.push(err23);
}
errors++;
}
}
if(data.apiVersion !== undefined){
let data2 = data.apiVersion;
if(!(((typeof data2 == "number") && (!(data2 % 1) && !isNaN(data2))) && (isFinite(data2)))){
const err24 = {instancePath:instancePath+"/apiVersion",schemaPath:"#/properties/apiVersion/type",keyword:"type",params:{type: "integer"},message:"must be integer"};
if(vErrors === null){
vErrors = [err24];
}
else {
vErrors.push(err24);
}
errors++;
}
if((typeof data2 == "number") && (isFinite(data2))){
if(data2 > 4294967295 || isNaN(data2)){
const err25 = {instancePath:instancePath+"/apiVersion",schemaPath:"#/properties/apiVersion/maximum",keyword:"maximum",params:{comparison: "<=", limit: 4294967295},message:"must be <= 4294967295"};
if(vErrors === null){
vErrors = [err25];
}
else {
vErrors.push(err25);
}
errors++;
}
if(data2 < 0 || isNaN(data2)){
const err26 = {instancePath:instancePath+"/apiVersion",schemaPath:"#/properties/apiVersion/minimum",keyword:"minimum",params:{comparison: ">=", limit: 0},message:"must be >= 0"};
if(vErrors === null){
vErrors = [err26];
}
else {
vErrors.push(err26);
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
if(data.commands !== undefined){
let data4 = data.commands;
if(Array.isArray(data4)){
if(data4.length > 16){
const err27 = {instancePath:instancePath+"/commands",schemaPath:"#/properties/commands/maxItems",keyword:"maxItems",params:{limit: 16},message:"must NOT have more than 16 items"};
if(vErrors === null){
vErrors = [err27];
}
else {
vErrors.push(err27);
}
errors++;
}
if(data4.length < 1){
const err28 = {instancePath:instancePath+"/commands",schemaPath:"#/properties/commands/minItems",keyword:"minItems",params:{limit: 1},message:"must NOT have fewer than 1 items"};
if(vErrors === null){
vErrors = [err28];
}
else {
vErrors.push(err28);
}
errors++;
}
const len1 = data4.length;
for(let i1=0; i1<len1; i1++){
if(!(validate23(data4[i1], {instancePath:instancePath+"/commands/" + i1,parentData:data4,parentDataProperty:i1,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate23.errors : vErrors.concat(validate23.errors);
errors = vErrors.length;
}
}
}
else {
const err29 = {instancePath:instancePath+"/commands",schemaPath:"#/properties/commands/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err29];
}
else {
vErrors.push(err29);
}
errors++;
}
}
if(data.description !== undefined){
let data6 = data.description;
if((typeof data6 !== "string") && (data6 !== null)){
const err30 = {instancePath:instancePath+"/description",schemaPath:"#/properties/description/type",keyword:"type",params:{type: schema31.properties.description.type},message:"must be string,null"};
if(vErrors === null){
vErrors = [err30];
}
else {
vErrors.push(err30);
}
errors++;
}
}
if(data.minimumHostVersion !== undefined){
if(typeof data.minimumHostVersion !== "string"){
const err31 = {instancePath:instancePath+"/minimumHostVersion",schemaPath:"#/properties/minimumHostVersion/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err31];
}
else {
vErrors.push(err31);
}
errors++;
}
}
if(data.name !== undefined){
if(typeof data.name !== "string"){
const err32 = {instancePath:instancePath+"/name",schemaPath:"#/properties/name/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err32];
}
else {
vErrors.push(err32);
}
errors++;
}
}
if(data.network !== undefined){
if(!(validate25(data.network, {instancePath:instancePath+"/network",parentData:data,parentDataProperty:"network",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate25.errors : vErrors.concat(validate25.errors);
errors = vErrors.length;
}
}
if(data.panel !== undefined){
let data10 = data.panel;
const _errs33 = errors;
let valid10 = false;
const _errs34 = errors;
if(!(validate29(data10, {instancePath:instancePath+"/panel",parentData:data,parentDataProperty:"panel",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate29.errors : vErrors.concat(validate29.errors);
errors = vErrors.length;
}
var _valid2 = _errs34 === errors;
valid10 = valid10 || _valid2;
const _errs35 = errors;
if(data10 !== null){
const err33 = {instancePath:instancePath+"/panel",schemaPath:"#/properties/panel/anyOf/1/type",keyword:"type",params:{type: "null"},message:"must be null"};
if(vErrors === null){
vErrors = [err33];
}
else {
vErrors.push(err33);
}
errors++;
}
var _valid2 = _errs35 === errors;
valid10 = valid10 || _valid2;
if(!valid10){
const err34 = {instancePath:instancePath+"/panel",schemaPath:"#/properties/panel/anyOf",keyword:"anyOf",params:{},message:"must match a schema in anyOf"};
if(vErrors === null){
vErrors = [err34];
}
else {
vErrors.push(err34);
}
errors++;
}
else {
errors = _errs33;
if(vErrors !== null){
if(_errs33){
vErrors.length = _errs33;
}
else {
vErrors = null;
}
}
}
}
if(data.permissions !== undefined){
let data11 = data.permissions;
if(Array.isArray(data11)){
const len2 = data11.length;
for(let i2=0; i2<len2; i2++){
let data12 = data11[i2];
if(typeof data12 !== "string"){
const err35 = {instancePath:instancePath+"/permissions/" + i2,schemaPath:"#/$defs/PublicPermission/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err35];
}
else {
vErrors.push(err35);
}
errors++;
}
if(!(((((((((((((((((((((data12 === "ui.window") || (data12 === "ui.panel")) || (data12 === "clipboard.write")) || (data12 === "speech.speak")) || (data12 === "clipboard.paste")) || (data12 === "files.openText")) || (data12 === "url.open")) || (data12 === "clipboard.read")) || (data12 === "selection.read")) || (data12 === "clipboard.history.read")) || (data12 === "clipboard.history.paste")) || (data12 === "clipboard.history.copy")) || (data12 === "network.https")) || (data12 === "credentials.youdaoTranslation")) || (data12 === "credentials.baiduOcr")) || (data12 === "credentials.deepseek")) || (data12 === "files.userSelected")) || (data12 === "files.index.readAll")) || (data12 === "notifications.publish")) || (data12 === "timer.control")) || (data12 === "background.schedule"))){
const err36 = {instancePath:instancePath+"/permissions/" + i2,schemaPath:"#/$defs/PublicPermission/enum",keyword:"enum",params:{allowedValues: schema44.enum},message:"must be equal to one of the allowed values"};
if(vErrors === null){
vErrors = [err36];
}
else {
vErrors.push(err36);
}
errors++;
}
}
}
else {
const err37 = {instancePath:instancePath+"/permissions",schemaPath:"#/properties/permissions/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err37];
}
else {
vErrors.push(err37);
}
errors++;
}
}
if(data.pluginId !== undefined){
if(typeof data.pluginId !== "string"){
const err38 = {instancePath:instancePath+"/pluginId",schemaPath:"#/properties/pluginId/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err38];
}
else {
vErrors.push(err38);
}
errors++;
}
}
if(data.runtime !== undefined){
let data14 = data.runtime;
if(data14 && typeof data14 == "object" && !Array.isArray(data14)){
if(data14.entry === undefined){
const err39 = {instancePath:instancePath+"/runtime",schemaPath:"#/$defs/PublicRuntimeV1/required",keyword:"required",params:{missingProperty: "entry"},message:"must have required property '"+"entry"+"'"};
if(vErrors === null){
vErrors = [err39];
}
else {
vErrors.push(err39);
}
errors++;
}
for(const key1 in data14){
if(!((key1 === "entry") || (key1 === "resultActions"))){
const err40 = {instancePath:instancePath+"/runtime",schemaPath:"#/$defs/PublicRuntimeV1/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key1},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err40];
}
else {
vErrors.push(err40);
}
errors++;
}
}
if(data14.entry !== undefined){
if(typeof data14.entry !== "string"){
const err41 = {instancePath:instancePath+"/runtime/entry",schemaPath:"#/$defs/PublicRuntimeV1/properties/entry/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err41];
}
else {
vErrors.push(err41);
}
errors++;
}
}
if(data14.resultActions !== undefined){
if(typeof data14.resultActions !== "boolean"){
const err42 = {instancePath:instancePath+"/runtime/resultActions",schemaPath:"#/$defs/PublicRuntimeV1/properties/resultActions/type",keyword:"type",params:{type: "boolean"},message:"must be boolean"};
if(vErrors === null){
vErrors = [err42];
}
else {
vErrors.push(err42);
}
errors++;
}
}
}
else {
const err43 = {instancePath:instancePath+"/runtime",schemaPath:"#/$defs/PublicRuntimeV1/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err43];
}
else {
vErrors.push(err43);
}
errors++;
}
}
if(data.schemaVersion !== undefined){
let data17 = data.schemaVersion;
if(!(((typeof data17 == "number") && (!(data17 % 1) && !isNaN(data17))) && (isFinite(data17)))){
const err44 = {instancePath:instancePath+"/schemaVersion",schemaPath:"#/properties/schemaVersion/type",keyword:"type",params:{type: "integer"},message:"must be integer"};
if(vErrors === null){
vErrors = [err44];
}
else {
vErrors.push(err44);
}
errors++;
}
if((typeof data17 == "number") && (isFinite(data17))){
if(data17 > 4294967295 || isNaN(data17)){
const err45 = {instancePath:instancePath+"/schemaVersion",schemaPath:"#/properties/schemaVersion/maximum",keyword:"maximum",params:{comparison: "<=", limit: 4294967295},message:"must be <= 4294967295"};
if(vErrors === null){
vErrors = [err45];
}
else {
vErrors.push(err45);
}
errors++;
}
if(data17 < 0 || isNaN(data17)){
const err46 = {instancePath:instancePath+"/schemaVersion",schemaPath:"#/properties/schemaVersion/minimum",keyword:"minimum",params:{comparison: ">=", limit: 0},message:"must be >= 0"};
if(vErrors === null){
vErrors = [err46];
}
else {
vErrors.push(err46);
}
errors++;
}
}
}
if(data.settings !== undefined){
let data18 = data.settings;
if(Array.isArray(data18)){
const len3 = data18.length;
for(let i3=0; i3<len3; i3++){
if(!(validate31(data18[i3], {instancePath:instancePath+"/settings/" + i3,parentData:data18,parentDataProperty:i3,rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate31.errors : vErrors.concat(validate31.errors);
errors = vErrors.length;
}
}
}
else {
const err47 = {instancePath:instancePath+"/settings",schemaPath:"#/properties/settings/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err47];
}
else {
vErrors.push(err47);
}
errors++;
}
}
if(data.share !== undefined){
if(!(validate33(data.share, {instancePath:instancePath+"/share",parentData:data,parentDataProperty:"share",rootData,dynamicAnchors}))){
vErrors = vErrors === null ? validate33.errors : vErrors.concat(validate33.errors);
errors = vErrors.length;
}
}
if(data.supportedPlatforms !== undefined){
let data21 = data.supportedPlatforms;
if(Array.isArray(data21)){
const len4 = data21.length;
for(let i4=0; i4<len4; i4++){
let data22 = data21[i4];
if(typeof data22 !== "string"){
const err48 = {instancePath:instancePath+"/supportedPlatforms/" + i4,schemaPath:"#/$defs/PublicPlatform/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err48];
}
else {
vErrors.push(err48);
}
errors++;
}
if(!((data22 === "windows") || (data22 === "macos"))){
const err49 = {instancePath:instancePath+"/supportedPlatforms/" + i4,schemaPath:"#/$defs/PublicPlatform/enum",keyword:"enum",params:{allowedValues: schema50.enum},message:"must be equal to one of the allowed values"};
if(vErrors === null){
vErrors = [err49];
}
else {
vErrors.push(err49);
}
errors++;
}
}
}
else {
const err50 = {instancePath:instancePath+"/supportedPlatforms",schemaPath:"#/properties/supportedPlatforms/type",keyword:"type",params:{type: "array"},message:"must be array"};
if(vErrors === null){
vErrors = [err50];
}
else {
vErrors.push(err50);
}
errors++;
}
}
if(data.version !== undefined){
if(typeof data.version !== "string"){
const err51 = {instancePath:instancePath+"/version",schemaPath:"#/properties/version/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err51];
}
else {
vErrors.push(err51);
}
errors++;
}
}
if(data.window !== undefined){
let data24 = data.window;
const _errs66 = errors;
let valid21 = false;
const _errs67 = errors;
if(data24 && typeof data24 == "object" && !Array.isArray(data24)){
if(data24.entry === undefined){
const err52 = {instancePath:instancePath+"/window",schemaPath:"#/$defs/PublicWindowV1/required",keyword:"required",params:{missingProperty: "entry"},message:"must have required property '"+"entry"+"'"};
if(vErrors === null){
vErrors = [err52];
}
else {
vErrors.push(err52);
}
errors++;
}
for(const key2 in data24){
if(!((key2 === "entry") || (key2 === "runtimeActions"))){
const err53 = {instancePath:instancePath+"/window",schemaPath:"#/$defs/PublicWindowV1/additionalProperties",keyword:"additionalProperties",params:{additionalProperty: key2},message:"must NOT have additional properties"};
if(vErrors === null){
vErrors = [err53];
}
else {
vErrors.push(err53);
}
errors++;
}
}
if(data24.entry !== undefined){
if(typeof data24.entry !== "string"){
const err54 = {instancePath:instancePath+"/window/entry",schemaPath:"#/$defs/PublicWindowV1/properties/entry/type",keyword:"type",params:{type: "string"},message:"must be string"};
if(vErrors === null){
vErrors = [err54];
}
else {
vErrors.push(err54);
}
errors++;
}
}
if(data24.runtimeActions !== undefined){
if(typeof data24.runtimeActions !== "boolean"){
const err55 = {instancePath:instancePath+"/window/runtimeActions",schemaPath:"#/$defs/PublicWindowV1/properties/runtimeActions/type",keyword:"type",params:{type: "boolean"},message:"must be boolean"};
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
const err56 = {instancePath:instancePath+"/window",schemaPath:"#/$defs/PublicWindowV1/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err56];
}
else {
vErrors.push(err56);
}
errors++;
}
var _valid3 = _errs67 === errors;
valid21 = valid21 || _valid3;
const _errs75 = errors;
if(data24 !== null){
const err57 = {instancePath:instancePath+"/window",schemaPath:"#/properties/window/anyOf/1/type",keyword:"type",params:{type: "null"},message:"must be null"};
if(vErrors === null){
vErrors = [err57];
}
else {
vErrors.push(err57);
}
errors++;
}
var _valid3 = _errs75 === errors;
valid21 = valid21 || _valid3;
if(!valid21){
const err58 = {instancePath:instancePath+"/window",schemaPath:"#/properties/window/anyOf",keyword:"anyOf",params:{},message:"must match a schema in anyOf"};
if(vErrors === null){
vErrors = [err58];
}
else {
vErrors.push(err58);
}
errors++;
}
else {
errors = _errs66;
if(vErrors !== null){
if(_errs66){
vErrors.length = _errs66;
}
else {
vErrors = null;
}
}
}
}
}
else {
const err59 = {instancePath,schemaPath:"#/type",keyword:"type",params:{type: "object"},message:"must be object"};
if(vErrors === null){
vErrors = [err59];
}
else {
vErrors.push(err59);
}
errors++;
}
validate20.errors = vErrors;
return errors === 0;
}
validate20.evaluated = {"props":true,"dynamicProps":false,"dynamicItems":false};

